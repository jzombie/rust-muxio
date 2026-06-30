<div align="center">
    <img src="https://raw.githubusercontent.com/jzombie/rust-muxio/main/assets/Muxio-logo.svg" width=250 height=250 />
</div>

<div align="center">
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Made%20with-Rust-black?&logo=Rust" alt="Made with Rust"></a>
  <a href="https://crates.io/crates/muxio"><img src="https://img.shields.io/crates/v/muxio.svg" alt="crates.io"></a>
  <a href="https://docs.rs/muxio"><img src="https://docs.rs/muxio/badge.svg" alt="Documentation"></a>
  <a href="https://deepwiki.com/jzombie/rust-muxio"><img src="https://deepwiki.com/badge.svg" alt="DeepWiki"></a>
  <a href="https://github.com/jzombie/rust-muxio/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="Apache 2.0 License"></a>
  <a href="https://coveralls.io/github/jzombie/rust-muxio?branch=main"><img src="https://coveralls.io/repos/github/jzombie/rust-muxio/badge.svg?branch=main" alt="Coverage Status"></a>
</div>

<p align="center"><strong>Examples:</strong> <a href="#websocket-usage-example">WebSocket RPC</a> · <a href="#wasm-websocket-rpc">WASM WebSocket RPC</a> · <a href="#ipc-usage-example">IPC RPC</a> · <a href="#streaming-rpc-example">Streaming RPC</a> · <a href="#handling-streaming-requests-on-the-server">Streaming Handlers</a> · <a href="#concurrent-bidirectional-streaming">Bidirectional Streaming</a></p>

# Muxio: A High-Performance Multiplexing and RPC Framework for Rust

**DRAFT -- WORK IN PROGRESS**

Muxio provides a robust and flexible foundation for building high-performance, transport-agnostic, and runtime-agnostic services in Rust. It offers a layered architecture that cleanly separates low-level binary stream multiplexing from high-level RPC logic, enabling you to create custom-tailored communication protocols.

## What is Muxio?

At its core, Muxio is a set of layered components that enable multiplexed data transmission over a single, unified connection. Think of it as a toolkit for managing multiple, independent data streams, such as RPC calls, file transfers, and real-time data feeds—without the overhead of multiple connections.

On top of this multiplexing layer, Muxio offers a minimal, unopinionated RPC framework. While you can use it directly, it's often more practical to leverage the provided extensions, which offer ready-to-use solutions for common environments (such as `tokio` or `WASM`).

----------

## Key Features

- **Efficient Multiplexing**: Multiple concurrent data streams over a single connection, correctly reassembling interleaved frames.

- **Streaming RPC**: Half-duplex request streams with full payload chunking — send large payloads incrementally without blocking other streams.

- **Bidirectional Streaming**: True concurrent streams in both directions using independent unidirectional streams — each direction is separately cancellable and independently backpressured.

- **Prebuffered (Unary) RPC**: Standard request/response RPC calls where the entire request is buffered before handler invocation.

- **Compile-Time Method IDs**: Deterministic `u64` identifiers via `rpc_method_id!("name")` macro using xxHash3 at compile time — no runtime cost, no magic numbers, platform-independent.

- **Minimalist RPC Layer**: Lightweight RPC on top of framing, giving you freedom to choose your own serialization formats, dispatching logic, and error-handling strategies.

- **Low-Overhead Binary Protocol**: Compact binary framing protocol with 17 bytes of header overhead per frame (stream ID, sequence ID, frame kind, timestamp).

- **Transport and Runtime Agnostic**: Core logic uses a flexible, callback-driven design, enabling seamless adaptation across Tokio, WASM, and standard library environments.

- **Disconnect Detection**: Three-layer mechanism — transport heartbeats (5s ping interval, 15s timeout), `fail_all_pending_requests()` on disconnect, and frame-level Cancel/End processing.

- **Extensible by Design:** Muxio comes with pre-built extensions:

  - **Tokio-based WebSocket [Server](https://github.com/jzombie/rust-muxio/tree/main/extensions/muxio-tokio-rpc-server/)/[Client](https://github.com/jzombie/rust-muxio/tree/main/extensions/muxio-tokio-rpc-client/)**: For native, multi-threaded environments.
  - **[WASM-based Web Client](https://github.com/jzombie/rust-muxio/tree/main/extensions/muxio-wasm-rpc-client/)**: For seamless integration into web applications via a JavaScript byte-passing bridge.
  - **Tokio-based IPC [Server](https://github.com/jzombie/rust-muxio/tree/main/extensions/muxio-tokio-rpc-ipc-server/)/[Client](https://github.com/jzombie/rust-muxio/tree/main/extensions/muxio-tokio-rpc-ipc-client/)**: For local inter-process communication over Unix domain sockets or Windows named pipes.

## How Muxio Compares

**Minimal framing overhead:** 17 bytes per frame (stream ID, sequence ID, frame kind, timestamp) vs HTTP/2's 9 bytes or gRPC's ~50+ bytes per protobuf message. For high-frequency small messages — keystrokes, mouse events, terminal output chunks — this overhead difference is significant.

**Transport-agnostic core:** The [`RpcServiceCallerInterface`](https://github.com/jzombie/rust-muxio/blob/main/extensions/muxio-rpc-service-caller/src/caller_interface.rs) trait abstracts away the transport so the same application code works over WebSocket, Unix domain sockets, or WASM bridges without modification. Not many (if any) other Rust RPC frameworks offer WASM as a first-class transport.

**FFI-friendly byte model:** The core dispatcher receives and emits raw byte slices, making it straightforward to bridge to C, C++, Python, or JavaScript. The included WASM client demonstrates this pattern with `#[wasm_bindgen]`.

**No protobuf dependency:** You choose your serialization — bitcode, bincode, manual encoding, or anything else. The framework handles framing and dispatch; it doesn't mandate a schema format.

**Tradeoffs:**

- **No built-in backpressure or flow control.** The write channel between encoder and transport I/O is unbounded by design — switching to a bounded channel without per-stream flow control (like HTTP/2 `WINDOW_UPDATE`) would cause head-of-line blocking. Under sustained producer > consumer load, memory can grow. Real applications should either size their chunks conservatively or implement application-level backpressure.

> _Note: The proper fix is likely per-stream byte budgets at the encoder. When a stream exceeds its budget, `write_bytes` pauses that stream's frames without blocking others going through the same channel. The framing layer already has `stream_id` on every frame; what's missing is the budget tracking. Not implemented yet._

- **No service discovery, load balancing, TLS, or auth.** These are left entirely to the user. gRPC and Tonic ship them out of the box.
- **Smaller ecosystem.** Muxio has one primary author. Tonic/gRPC have broad adoption, protobuf tooling, interceptors, and reflection.

**Other Notes:**

Muxio is designed to be compiled into Rust first. Interop with other languages happens through FFI (PyO3 for Python, `#[wasm_bindgen]` for JavaScript, C ABI for other languages) — you embed the Rust core rather than reimplementing the protocol from a spec. This is the same model as `libnghttp2` or `libssl`: the library is consumed as a compiled dependency, not implemented independently.

## How Muxio Compares

**Minimal framing overhead:** 17 bytes per frame (stream ID, sequence ID, frame kind, timestamp) vs HTTP/2's 9 bytes or gRPC's ~50+ bytes per protobuf message. For high-frequency small messages — keystrokes, mouse events, terminal output chunks — this overhead difference is significant.

**Transport-agnostic core:** The [`RpcServiceCallerInterface`](https://github.com/jzombie/rust-muxio/blob/main/extensions/muxio-rpc-service-caller/src/caller_interface.rs) trait abstracts away the transport so the same application code works over WebSocket, Unix domain sockets, or WASM bridges without modification. Not many (if any) other Rust RPC frameworks offer WASM as a first-class transport.

**FFI-friendly byte model:** The core dispatcher receives and emits raw byte slices, making it straightforward to bridge to C, C++, Python, or JavaScript. The included WASM client demonstrates this pattern with `#[wasm_bindgen]`.

**No protobuf dependency:** You choose your serialization — bitcode, bincode, manual encoding, or anything else. The framework handles framing and dispatch; it doesn't mandate a schema format.

## Core Use Cases & Design Philosophy

Muxio is engineered to solve specific challenges in building modern, distributed systems. Its architecture and features are guided by the following principles:

- **Low-Latency, High-Performance Communication**: Muxio is built for speed. It uses a compact, **low-overhead binary protocol** (instead of text-based formats like JSON). This significantly reduces the size of data sent over the network and minimizes the CPU cycles needed for serialization and deserialization. By avoiding complex parsing, Muxio lowers end-to-end latency, making it well-suited for real-time applications such as financial data streaming, multiplayer games, and interactive remote tooling.

- **Cross-Platform Code with Agnostic Frontends**: Write your core application logic once and deploy it across multiple platforms. Muxio achieves this through its generic [`RpcServiceCallerInterface` trait](https://github.com/jzombie/rust-muxio/blob/main/extensions/muxio-rpc-service-caller/src/caller_interface.rs), which abstracts away the underlying transport. The same application code that calls an RPC method using the native [`RpcClient`](https://github.com/jzombie/rust-muxio/tree/main/extensions/muxio-tokio-rpc-client/) can also be utilized in a browser with the [`RpcWasmClient`](https://github.com/jzombie/rust-muxio/tree/main/extensions/muxio-wasm-rpc-client/) with minimal changes, while additional client types can also be added, provided they implement the same aformentioned `RpcServiceCallerInterface`. This design ensures that improvements to the core service logic benefit all clients simultaneously, even custom-built clients.

- **Shared Service Definitions for Type-Safe APIs**: Enforce integrity between your server and client by defining RPC methods, inputs, and outputs in a shared crate. By implementing the [`RpcMethodPrebuffered` trait](https://github.com/jzombie/rust-muxio/tree/main/extensions/muxio-rpc-service-caller/src/prebuffered/) , both client and server depend on a single source of truth for the API contract. This completely eliminates a common class of runtime errors, as any mismatch in data structures between the client and server will result in a compile-time error.

- **A Strong Foundation for Foreign Function Interfaces (FFI)**: The framework's byte-oriented design makes it an ideal foundation for bridging Rust with other languages. Because the core dispatcher only needs to receive and emit byte slices, you can easily create an FFI layer that connects Muxio to C, C++, Swift, or any language that can handle byte array (including Python). The included [`muxio-wasm-rpc-client`](https://github.com/jzombie/rust-muxio/tree/main/extensions/muxio-wasm-rpc-client/) serves as a perfect example, using `wasm_bindgen` to create a simple bridge between the Rust client and the JavaScript host environment.

## Installation

For Muxio's core:

```sh
cargo add muxio
```

This provides the low-level functionality, but [Muxio extensions](https://github.com/jzombie/rust-muxio/tree/main/extensions/) are likely more desirable for most use cases.

## WebSocket Usage Example

Let's build a simple sample app which spins up a Tokio-based WebSocket server, adds some routes, then spins up a client, performs some requests, then shuts everything down.

This example code was taken from the [`example-muxio-ws-rpc-app`](https://github.com/jzombie/rust-muxio/tree/main/examples/example-muxio-ws-rpc-app/) crate.

```rust
use example_muxio_rpc_service_definition::{
    RpcMethodPrebuffered,
    prebuffered::{Add, Echo, Mult},
};
use muxio_tokio_rpc_client::{
    RpcCallPrebuffered, RpcClient, RpcServiceCallerInterface, RpcTransportState,
};
use muxio_tokio_rpc_server::{RpcServer, RpcServiceEndpointInterface, utils::tcp_listener_to_host_port};
use std::sync::Arc;
use tokio::join;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    // Bind to a random available port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    
    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();

    {
        let server = Arc::new(RpcServer::new(None));
        let endpoint = server.endpoint();

        let _ = join!(
            endpoint.register_prebuffered(Add::METHOD_ID, |request_bytes: Vec<u8>, _ctx| async move {
                let request_params = Add::decode_request(&request_bytes)?;
                let sum = request_params.iter().sum();
                let response_bytes = Add::encode_response(sum)?;
                Ok(response_bytes)
            }),
            endpoint.register_prebuffered(Mult::METHOD_ID, |request_bytes: Vec<u8>, _ctx| async move {
                let request_params = Mult::decode_request(&request_bytes)?;
                let product = request_params.iter().product();
                let response_bytes = Mult::encode_response(product)?;
                Ok(response_bytes)
            }),
            endpoint.register_prebuffered(Echo::METHOD_ID, |request_bytes: Vec<u8>, _ctx| async move {
                let request_params = Echo::decode_request(&request_bytes)?;
                let response_bytes = Echo::encode_response(request_params)?;
                Ok(response_bytes)
            })
        );

        // Spawn the server using the pre-bound listener
        let _server_task = tokio::spawn({
            // Clone the Arc for the server task
            let server = Arc::clone(&server);
            async move {
                let _ = server.serve_with_listener(listener).await;
            }
        });
    }

    // This block runs the client against the server
    {
        // Wait briefly for server to start
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Connect to the server
        let rpc_client = RpcClient::new(&server_host.to_string(), server_port).await?;

        rpc_client.set_state_change_handler(move |new_state: RpcTransportState| {
            // This code will run every time the connection state changes
            tracing::info!("[Callback] Transport state changed to: {:?}", new_state);
        }).await;

        // `join!` will await all responses before proceeding
        let (res1, res2, res3, res4, res5, res6) = join!(
            Add::call(&*rpc_client, vec![1.0, 2.0, 3.0]),
            Add::call(&*rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&*rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&*rpc_client, vec![1.5, 2.5, 8.5]),
            Echo::call(&*rpc_client, b"testing 1 2 3".into()),
            Echo::call(&*rpc_client, b"testing 4 5 6".into()),
        );

        assert_eq!(res1?, 6.0);
        assert_eq!(res2?, 18.0);
        assert_eq!(res3?, 168.0);
        assert_eq!(res4?, 31.875);
        assert_eq!(res5?, b"testing 1 2 3");
        assert_eq!(res6?, b"testing 4 5 6");

        Ok(())
    }
}
```

### WASM WebSocket RPC

The [WASM client](https://github.com/jzombie/rust-muxio/tree/main/extensions/muxio-wasm-rpc-client/) follows a callback-driven pattern — the browser owns the WebSocket, and Rust is called on events. The [`static_lib`](https://github.com/jzombie/rust-muxio/tree/main/extensions/muxio-wasm-rpc-client/src/static_lib/) module provides `#[wasm_bindgen]` exports that JS calls on `onopen`, `onmessage`, and `onclose`. The core [`RpcWasmClient`](https://github.com/jzombie/rust-muxio/blob/main/extensions/muxio-wasm-rpc-client/src/rpc_wasm_client.rs) implements the same `RpcServiceCallerInterface` used above, so calling methods like `Add::call(...)` works identically in the browser.

## IPC Usage Example

The same application code works over Unix domain sockets or Windows named pipes via the IPC transport. Only the client and server types change — the service definitions (`Add`, `Mult`, `Echo`) are identical to the WebSocket example above.

This example code was taken from the [`example-muxio-ws-rpc-app`](https://github.com/jzombie/rust-muxio/tree/main/examples/example-muxio-ws-rpc-app/) crate.

```rust
use example_muxio_rpc_service_definition::{
    RpcMethodPrebuffered,
    prebuffered::{Add, Echo, Mult},
};
use muxio_tokio_rpc_ipc_client::{RpcIpcClient, RpcCallPrebuffered, RpcServiceCallerInterface, RpcTransportState};
use muxio_tokio_rpc_ipc_server::{RpcIpcServer, RpcServiceEndpointInterface};
use tokio::join;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    // Use process ID to avoid collisions between concurrent test invocations
    let socket_name = format!("muxio-ipc-example-{}", std::process::id());

    // This block sets up and spawns the server
    {
        let server = RpcIpcServer::new(None);
        let endpoint = server.endpoint();

        // Register server methods on the endpoint
        let _ = join!(
            endpoint.register_prebuffered(Add::METHOD_ID, |request_bytes: Vec<u8>, _ctx| async move {
                let request_params = Add::decode_request(&request_bytes)?;
                let sum = request_params.iter().sum();
                let response_bytes = Add::encode_response(sum)?;
                Ok(response_bytes)
            }),
            endpoint.register_prebuffered(Mult::METHOD_ID, |request_bytes: Vec<u8>, _ctx| async move {
                let request_params = Mult::decode_request(&request_bytes)?;
                let product = request_params.iter().product();
                let response_bytes = Mult::encode_response(product)?;
                Ok(response_bytes)
            }),
            endpoint.register_prebuffered(Echo::METHOD_ID, |request_bytes: Vec<u8>, _ctx| async move {
                let request_params = Echo::decode_request(&request_bytes)?;
                let response_bytes = Echo::encode_response(request_params)?;
                Ok(response_bytes)
            })
        );

        // Spawn the server; it runs forever in the background
        let server_name = socket_name.clone();
        tokio::spawn(async move {
            let _ = server.serve(&server_name).await;
        });
    }

    // This block runs the client against the server
    {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let rpc_client = RpcIpcClient::new(&socket_name).await?;

        rpc_client
            .set_state_change_handler(move |new_state: RpcTransportState| {
                tracing::info!("[Callback] Transport state changed to: {:?}", new_state);
            })
            .await;

        let (res1, res2, res3, res4, res5, res6) = join!(
            Add::call(&*rpc_client, vec![1.0, 2.0, 3.0]),
            Add::call(&*rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&*rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&*rpc_client, vec![1.5, 2.5, 8.5]),
            Echo::call(&*rpc_client, b"testing 1 2 3".into()),
            Echo::call(&*rpc_client, b"testing 4 5 6".into()),
        );

        assert_eq!(res1?, 6.0);
        assert_eq!(res2?, 18.0);
        assert_eq!(res3?, 168.0);
        assert_eq!(res4?, 31.875);
        assert_eq!(res5?, b"testing 1 2 3");
        assert_eq!(res6?, b"testing 4 5 6");

        Ok(())
    }
}
```

## Streaming RPC Example

Muxio supports streaming requests over any transport. Each stream is **half-duplex**: the sender writes chunks and ends the stream, then reads the single response. True bidirectional messaging is achieved with **two independent concurrent streams** (one per direction).

### Streaming a request from the client

```rust
use futures::StreamExt;
use muxio_core::rpc::RpcRequest;
use muxio_rpc_service::rpc_method_id;
use muxio_rpc_service_caller::dynamic_channel::DynamicChannelType;
use muxio_tokio_rpc_client::{RpcClient, RpcServiceCallerInterface};

// Method IDs are generated at compile time via xxHash3 from string names.
// Each application defines its own — use distinct names for distinct methods.
const STREAM_INPUT_METHOD_ID: u64 = rpc_method_id!("example.stream_input");

async fn streaming_example(rpc_client: &RpcClient) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let large_payload = vec![42u8; 100_000];

    let request = RpcRequest {
        rpc_method_id: STREAM_INPUT_METHOD_ID,
        rpc_param_bytes: None,
        rpc_prebuffered_payload_bytes: None,
        is_finalized: false,
    };

    let (mut encoder, mut receiver) = rpc_client
        .call_rpc_streaming(request, DynamicChannelType::Unbounded)
        .await?;

    for chunk in large_payload.chunks(4096) {
        encoder.write_bytes(chunk)?;
    }
    encoder.flush()?;
    encoder.end_stream()?;

    // Read the single response from the server
    let mut response = Vec::new();
    while let Some(chunk) = receiver.next().await {
        match chunk {
            Ok(bytes) => response.extend_from_slice(&bytes),
            Err(e) => eprintln!("stream error: {e:?}"),
        }
    }
    Ok(())
}
```

### Handling streaming requests on the server

Streaming handlers are registered via `register_stream_handler()` and receive individual `RpcStreamEvent`s as they arrive from the transport. Unlike prebuffered handlers (which accumulate the entire request into a `Vec<u8>` before invoking the handler), streaming handlers are called synchronously for each event:

```rust
use muxio_core::rpc::rpc_internals::RpcStreamEvent;
use muxio_rpc_service::rpc_method_id;
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};

const STREAM_INPUT_METHOD_ID: u64 = rpc_method_id!("example.stream_input");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let endpoint = RpcServiceEndpoint::<()>::new();

        endpoint.register_stream_handler(STREAM_INPUT_METHOD_ID, |event, _emit, _ctx| {
            match event {
                RpcStreamEvent::Header { rpc_method_id, .. } => {
                    println!("Stream started for method {rpc_method_id}");
                }
                RpcStreamEvent::PayloadChunk { bytes, .. } => {
                    println!("Received {} bytes", bytes.len());
                }
                RpcStreamEvent::End { .. } => {
                    println!("Stream complete");
                }
                RpcStreamEvent::Error { frame_decode_error, .. } => {
                    eprintln!("Stream error: {frame_decode_error:?}");
                }
            }
        }).await?;
        Ok(())
    })
}
```

> **Note:** The second argument (`_emit`) accepts a raw transport byte sink
> (`Box<dyn RpcEmit>`). A future API will expose `RpcDispatcher::respond()`
> for sending properly framed response chunks back to the caller.

### Client vs Server Streaming: Deliberate Asymmetry

The streaming API is intentionally asymmetric between the two roles:

- **Client side** (`call_rpc_streaming`): The client initiates a stream, writes chunks via an `RpcStreamEncoder`, and reads the response via a `DynamicReceiver` (which implements `Stream`). This is the producer/consumer pattern — the caller produces request data and consumes the response.

- **Server side** (`register_stream_handler`): The server registers a handler that is invoked
  synchronously for each `RpcStreamEvent` as it arrives. The handler receives a `StreamResponder` for sending response chunks back. This is the event-driven pattern — the server reacts to stream events rather than driving the stream.

The asymmetry is inherent to the request-reply model: one side initiates (client), the other handles (server). The same `RpcServiceCallerInterface` trait powers client-style calls from *any* context, including server-side code that wants to push data to connected clients (see "server-initiated calls" below).

### Disconnect Detection

Streaming RPC calls detect remote disconnection through three layers:

1. **Transport heartbeats**: The transport sends periodic pings (default 5s interval on the WebSocket server) and closes the connection if no response arrives within the timeout (15s).

2. **`fail_all_pending_requests()`**: When any transport detects a disconnect, it calls this method on the dispatcher, which fails all pending response handlers. Any    `receiver.next().await` in application code will return `None` or `Err(...)`.

3. **Frame-level signaling**: Individual `Cancel` and `End` frames are processed by the  stream decoder. Corrupt or invalid frames produce `RpcStreamEvent::Error`, which propagates as a transport error to the caller.

Applications should handle stream termination by checking the `Result` from receiver.next().await` and treating `None` (stream ended) or `Err(...)` as signals that the remote peer is gone. 

### Streaming from the server to the client (server-initiated calls)

Any handle that implements `RpcServiceCallerInterface` — such as the
`ConnectionContextHandle` obtained from a `ClientConnected` event —
can initiate streaming calls:

```rust
use muxio_core::rpc::RpcRequest;
use muxio_rpc_service::rpc_method_id;
use muxio_rpc_service_caller::dynamic_channel::DynamicChannelType;
use muxio_rpc_service_caller::RpcServiceCallerInterface;
use muxio_tokio_rpc_server::RpcServerEvent;

// Applications define their own method IDs — this is just an example.
const SERVER_STREAM_METHOD_ID: u64 = rpc_method_id!("example.server_stream");

async fn server_streaming_example(
    mut event_rx: tokio::sync::mpsc::UnboundedReceiver<RpcServerEvent>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pty_output: Vec<u8> = b"terminal output chunk".to_vec();

    while let Some(event) = event_rx.recv().await {
        if let RpcServerEvent::ClientConnected(handle) = event {
            let po = pty_output.clone();
            tokio::spawn(async move {
                let stream_request = RpcRequest {
                    rpc_method_id: SERVER_STREAM_METHOD_ID,
                    rpc_param_bytes: None,
                    rpc_prebuffered_payload_bytes: None,
                    is_finalized: false,
                };
                let (mut encoder, _receiver) = handle
                    .call_rpc_streaming(stream_request, DynamicChannelType::Unbounded)
                    .await?;
                encoder.write_bytes(&po)?;
                encoder.end_stream()?;
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
            });
        }
    }
    Ok(())
}
```

### Concurrent bidirectional streaming

Streams are multiplexed over a single connection. Each stream is unidirectional, so bidirectional communication uses two independent streams without a separate connection per direction. 

A single bidirectional stream would interleave both directions in one channel, coupling their backpressure, error handling, and lifecycle; two (or more) unidirectional streams keep each direction isolated and individually cancellable.

```text
Single connection, multiple streams.

Client                          Server
  │                               │
  ├─ call_rpc_streaming ────────► │     stream A open
  │ ◄──── call_rpc_streaming ─────│     stream B open
  │ ── chunk A ─────────────────► │
  │ ◄──── chunk B ─────────────── │     interleaved
  │ ── chunk A ─────────────────► │     writes
  │ ◄──── chunk B ─────────────── │
  │   ...                         │
  │ ── End A ───────────────────► │
  │ ◄──── End B ───────────────── │
  │ ◄──── response A (echo) ───── │
  │ ── response B (echo) ───────► │
```

This is exactly what the [`concurrent_bidirectional_streaming`](https://github.com/search?q=repo%3Ajzombie%2Frust-muxio+concurrent_bidirectional_streaming&type=code) integration test exercises. It spawns two `tokio::spawn` tasks that write chunks in opposite directions and **yield between each chunk** so the writes are truly interleaved at the application level, not buffered and sent in one burst per direction.

## License

Licensed under the [Apache-2.0 License](https://github.com/jzombie/rust-muxio/blob/main/LICENSE).
