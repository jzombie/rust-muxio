# Muxio RPC Service Endpoint

`muxio-rpc-service-endpoint` provides a powerful, transport-agnostic interface and a concrete implementation for the "server-side" of a Muxio RPC system. It is responsible for receiving raw byte data, decoding RPC requests, invoking the correct user-defined handlers, and encoding responses.

This crate is designed to be a foundational building block, allowing developers to integrate Muxio RPC handling into any async server framework (e.g., Axum, Hyper, or custom TCP/UDP listeners) with minimal effort.

## Core Concepts

The architecture of this crate is centered around a "smart trait" pattern, which promotes flexibility and code reuse.

### 1. The `RpcServiceEndpointInterface` Trait

This is the heart of the crate. It defines the full set of capabilities for a service endpoint, such as registering handlers and processing inbound data. The key design feature is its use of **default methods**. Any struct that wants to act as an endpoint only needs to implement two simple "getter" methods:

* `get_dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>>`
* `get_prebuffered_handlers(&self) -> Arc<Mutex<HashMap<...>>>`

By providing access to its state, the struct automatically inherits the complete, complex logic for `register_prebuffered` and `read_bytes` for free. This makes it trivial to integrate RPC handling into existing server structures.

### 2. The `RpcServiceEndpoint` Struct

This is the standard, concrete implementation of the `RpcServiceEndpointInterface`. For most use cases, you can simply create an instance of this struct and use it directly. It holds the `RpcDispatcher` and the `HashMap` of handlers, ready to be used by a higher-level server implementation.

### 3. Transport Agnosticism

The endpoint is completely decoupled from the network transport. The `read_bytes` method simply accepts a byte slice (`&[u8]`) and an `on_emit` closure. It has no knowledge of WebSockets, TCP, or any other protocol. This makes it highly versatile and reusable.

## Usage Example

Here is a typical workflow for using the `RpcServiceEndpoint` in a server context.

```rust
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
use std::sync::Arc;
use tokio::sync::mpsc;

// Assume `Add` is a defined RPC method structure.
use service_definition::math::Add; 

#[tokio::main]
async fn main() {
    // 1. Create a new endpoint. This is typically done once at server startup
    //    and shared across all connections via an Arc.
    let endpoint = Arc::new(RpcServiceEndpoint::new());

    // 2. Register a handler for a specific RPC method.
    //    This handler is an async closure that takes bytes and returns bytes.
    let registration_result = endpoint.register_prebuffered(
        Add::METHOD_ID, 
        |req_bytes| async move {
            // Decode the request from raw bytes into your structured type.
            let numbers_to_add = Add::decode_request(&req_bytes)?;

            // Perform the business logic.
            let sum: f64 = numbers_to_add.iter().sum();

            // Encode the response from your result type back into raw bytes.
            let response_bytes = Add::encode_response(sum)?;
            
            Ok(response_bytes)
        }
    ).await;

    assert!(registration_result.is_ok());

    // 3. In your network loop, when you receive data from a client...
    let incoming_data = Add::encode_request(vec![10.0, 20.0, 5.5]).unwrap();
    
    // Create a channel to capture the outbound data.
    let (tx, mut rx) = mpsc::unbounded_channel();

    // The `on_emit` closure defines how to send data back to the client.
    let on_emit = move |chunk: &[u8]| {
        let _ = tx.send(chunk.to_vec());
    };

    // 4. Process the bytes. The endpoint will invoke the correct handler.
    endpoint.read_bytes(&incoming_data, on_emit).await.unwrap();

    // 5. The response is received on our channel, ready to be sent over the wire.
    let response_chunk = rx.recv().await.unwrap();
    let response_value = Add::decode_response(&response_chunk).unwrap();

    println!("RPC call successful! Result: {}", response_value);
    assert_eq!(response_value, 35.5);
}
```

## Relationship to Other Crates

* **`muxio`**: Provides the low-level RPC primitives like `RpcDispatcher`, `RpcRequest`, and `RpcResponse` that this crate builds upon.
* **`muxio-rpc-service`**: Provides shared constants and helper traits for defining RPC methods.
* **`muxio-tokio-rpc-server`**: A higher-level crate that uses `muxio-rpc-service-endpoint` to provide a ready-to-use WebSocket server powered by Axum.

This crate serves as the essential middle layer, connecting the low-level Muxio protocol with high-level server applications.
