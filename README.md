<div align="center">
    <img src="./assets/Muxio-logo.svg" width=250 height=250 />
</div>

<div align="center">
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Made%20with-Rust-black?&logo=Rust" alt="made-with-rust"></a>
  <a href="https://crates.io/crates/muxio"><img src="https://img.shields.io/crates/v/muxio.svg" alt="crates.io"></a>
  <a href="https://docs.rs/muxio"><img src="https://docs.rs/muxio/badge.svg" alt="Documentation"></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="license"></a>
</div>

# Muxio: A High-Performance Multiplexing and RPC Framework for Rust

**DRAFT -- WORK IN PROGRESS**

Muxio provides a robust and flexible foundation for building high-performance, transport-agnostic, and runtime-agnostic services in Rust. It offers a layered architecture that cleanly separates low-level binary stream multiplexing from high-level RPC logic, enabling you to create custom-tailored communication protocols.

## What is Muxio?

At its core, Muxio is a set of layered components that enable multiplexed data transmission over a single, unified connection. Think of it as a toolkit for managing multiple, independent data streams, such as RPC calls, file transfers, and real-time data feeds—without the overhead of multiple connections.

On top of this multiplexing layer, Muxio offers a minimal, unopinionated RPC framework. While you can use it directly, it's often more practical to leverage the provided extensions, which offer ready-to-use solutions for common environments (such as `tokio` or `WASM`).

----------

## Key Features

- **Efficient Multiplexing**: Muxio's foundational framing protocol can reliably manage numerous concurrent data streams over a single connection, correctly reassembling interleaved and out-of-order frames.

- **Minimalist RPC Layer**: A lightweight RPC mechanism is provided on top of the framing layer, giving you the freedom to choose your own serialization formats, dispatching logic, and error-handling strategies.

- **Low-Overhead Binary Protocol**: Muxio uses a compact binary framing protocol to minimize data transmission overhead, making it highly efficient for performance-sensitive applications. All communication, from frame headers to RPC payloads, is handled as raw bytes. The protocol defines a minimal header structure to keep data transfer lean.

- **Transport and Runtime Agnostic**: The core logic uses a flexible, callback-driven design, enabling seamless adaptation across different environments. It supports both Tokio and standard library servers, as well as native and WASM clients, with or without Tokio.

- **Extensible by Design:** Muxio comes with pre-built extensions that demonstrate how to integrate the core library into real-world applications:.

  - **Tokio-based [Server](./extensions/muxio-tokio-rpc-server/)/[Client](./extensions/muxio-tokio-rpc-client/)]**: For native, multi-threaded environments.
  - **[WASM-based Web Client](./extensions/muxio-wasm-rpc-client/)**: For seamless integration into web applications, communicating with a JavaScript host via a simple byte-passing bridge.

## Core Use Cases & Design Philosophy

Muxio is engineered to solve specific challenges in building modern, distributed systems. Its architecture and features are guided by the following principles:

- **Low-Latency, High-Performance Communication**: Muxio is built for speed. It uses a compact, **low-overhead binary protocol** (instead of text-based formats like JSON). This significantly reduces the size of data sent over the network and minimizes the CPU cycles needed for serialization and deserialization. By avoiding complex parsing, Muxio lowers end-to-end latency, making it well-suited for real-time applications such as financial data streaming, multiplayer games, and interactive remote tooling.

- **Cross-Platform Code with Agnostic Frontends**: Write your core application logic once and deploy it across multiple platforms. Muxio achieves this through its generic [`RpcServiceCallerInterface` trait](./extensions/muxio-rpc-service-caller/src/caller_interface.rs), which abstracts away the underlying transport. The same application code that calls an RPC method can run on a native [`RpcClient`](./extensions/muxio-tokio-rpc-client/) using Tokio or a RpcWasmClient in a web browser with no changes, while additional client types can be added with minimal code, provided they implement the same aformentioned `RpcServiceCallerInterface`. This design ensures that improvements to the core service logic benefit all clients simultaneously, even custom-built clients.

- **Shared Service Definitions for Type-Safe APIs**: Enforce integrity between your server and client by defining RPC methods, inputs, and outputs in a shared crate. By implementing the [`RpcMethodPrebuffered` trait](./extensions/muxio-rpc-service-caller/src/prebuffered/) , both client and server depend on a single source of truth for the API contract. This completely eliminates a common class of runtime errors, as any mismatch in data structures between the client and server will result in a compile-time error.

- **A Strong Foundation for Foreign Function Interfaces (FFI)**: The framework's byte-oriented design makes it an ideal foundation for bridging Rust with other languages. Because the core dispatcher only needs to receive and emit byte slices, you can easily create an FFI layer that connects Muxio to C, C++, Swift, or any language that can handle byte array (including Python). The included [`muxio-wasm-rpc-client`](./extensions/muxio-wasm-rpc-client/) serves as a perfect example, using #[wasm_bindgen] to create a simple bridge between the Rust client and the JavaScript host environment.

## Installation

For Muxio's core:

```sh
cargo add muxio
```

This provides the low-level functionality, but [Muxio extensions](./extensions/) are likely more desirable for most use cases.

## Usage Example

Let's build a simple sample app which spins up a Tokio-based WebSocket server, adds some routes, then spins up a client, performs some requests, then shuts everything down.

This example code was taken from the [`example-muxio-ws-rpc-app`](./example-muxio-ws-rpc-app/) crate.

```rust
use example_muxio_rpc_service_definition::{
    RpcMethodPrebuffered,
    prebuffered::{Add, Echo, Mult},
};
use muxio_tokio_rpc_client::{RpcCallPrebuffered, RpcClient};
use muxio_tokio_rpc_server::{RpcServer, RpcServiceEndpointInterface};
use std::sync::Arc;
use tokio::join;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    // Bind to a random available port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    {
        let server = RpcServer::new();

        let endpoint = server.endpoint();

        // Register server method
        // Note: If not using `join!`, each `register` call must be awaited.
        let _ = join!(
            endpoint.register_prebuffered(Add::METHOD_ID, |_, bytes| async move {
                let params = Add::decode_request(&bytes)?;
                let sum = params.iter().sum();
                let response_bytes = Add::encode_response(sum)?;
                Ok(response_bytes)
            }),
            endpoint.register_prebuffered(Mult::METHOD_ID, |_, bytes| async move {
                let params = Mult::decode_request(&bytes)?;
                let product = params.iter().product();
                let response_bytes = Mult::encode_response(product)?;
                Ok(response_bytes)
            }),
            endpoint.register_prebuffered(Echo::METHOD_ID, |_, bytes| async move {
                let params = Echo::decode_request(&bytes)?;
                let response_bytes = Echo::encode_response(params)?;
                Ok(response_bytes)
            })
        );

        // Spawn the server using the pre-bound listener
        let _server_task = tokio::spawn({
            let server = server;
            async move {
                let _ = Arc::new(server).serve_with_listener(listener).await;
            }
        });
    }

    {
        // Wait briefly for server to start
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Use the actual bound address for the client
        let rpc_client = RpcClient::new(&format!("ws://{}/ws", addr)).await;

         // TODO: Implement
        // rpc_client.on_state_change(move |new_state| {
        //     // This code will run every time the connection state changes.
        //     println!("[Callback] Transport state changed to: {:?}", new_state);

        //     // Update the shared state variable.
        //     let mut state = state_clone.lock().unwrap();
        //     *state = new_state;
        // });

        // `join!` will await all responses before proceeding
        let (res1, res2, res3, res4, res5, res6) = join!(
            Add::call(&rpc_client, vec![1.0, 2.0, 3.0]),
            Add::call(&rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&rpc_client, vec![1.5, 2.5, 8.5]),
            Echo::call(&rpc_client, b"testing 1 2 3".into()),
            Echo::call(&rpc_client, b"testing 4 5 6".into()),
        );

        // Assert that all results are correct.
        assert_eq!(res1.unwrap(), 6.0);
        assert_eq!(res2.unwrap(), 18.0);
        assert_eq!(res3.unwrap(), 168.0);
        assert_eq!(res4.unwrap(), 31.875);
        assert_eq!(res5.unwrap(), b"testing 1 2 3");
        assert_eq!(res6.unwrap(), b"testing 4 5 6");
    }
}
```

## License

Licensed under the [Apache-2.0 License](./LICENSE).
