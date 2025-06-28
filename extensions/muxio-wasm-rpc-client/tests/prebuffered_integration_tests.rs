//! This integration test simulates a full client-server roundtrip for the WASM client.
//!
//! To achieve a realistic test environment that closely mirrors the native Tokio tests,
//! this setup involves three main components:
//!
//! 1.  **A Real `RpcServer`**: An actual `muxio-tokio-rpc-server` instance is spawned
//!     in a Tokio task, listening on a random TCP port. This is the same server
//!     used in the native integration tests.
//!
//! 2.  **The `RpcWasmClient`**: The WebAssembly client under test. It is designed
//!     to be runtime-agnostic and sends its output bytes to a callback.
//!
//! 3.  **A WebSocket Bridge**: A small, lightweight bridge created specifically for this
//!     test. It connects to the `RpcServer` via a real WebSocket connection. It then
//!     forwards bytes between the `RpcWasmClient`'s callback/dispatcher and the
//!     WebSocket, effectively linking the two components over a real network socket.
//!
//! This approach ensures that the `RpcWasmClient` is tested against the actual
//! server implementation, providing a high-fidelity integration test.

use example_muxio_rpc_service_definition::prebuffered::{Add, Echo, Mult};
use futures_util::{SinkExt, StreamExt};
use muxio_rpc_service::{
    constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE, prebuffered::RpcMethodPrebuffered,
};
use muxio_rpc_service_caller::RpcServiceCallerInterface;
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_tokio_rpc_server::{RpcServer, RpcServiceEndpointInterface};
use muxio_wasm_rpc_client::RpcWasmClient;
use std::sync::Arc;
use tokio::join;
use tokio::net::TcpListener;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::task;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

#[tokio::test]
async fn test_success_client_server_roundtrip() {
    // 1. Start a real Tokio-based RPC Server
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("ws://{addr}/ws");

    // Wrap server in an Arc immediately to manage ownership correctly.
    let server = Arc::new(RpcServer::new());
    let endpoint = server.endpoint(); // Get endpoint for registration

    // Register handlers on the server.
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

    // Spawn the server to run in the background.
    tokio::spawn({
        // Clone the Arc for the spawned server task.
        let server = Arc::clone(&server);
        async move {
            let _ = server.serve_with_listener(listener).await;
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // 2. Create the WASM client instance.
    let (to_bridge_tx, mut to_bridge_rx) = tokio_mpsc::unbounded_channel::<Vec<u8>>();
    let client = Arc::new(RpcWasmClient::new(move |bytes| {
        to_bridge_tx.send(bytes).unwrap();
    }));

    // 3. Setup the WebSocket bridge.
    let (ws_stream, _) = connect_async(&server_url)
        .await
        .expect("Failed to connect to server");
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // This task is fine as it only deals with async channels.
    tokio::spawn(async move {
        while let Some(bytes) = to_bridge_rx.recv().await {
            if ws_sender
                .send(WsMessage::Binary(bytes.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    tokio::spawn({
        let client = client.clone();
        async move {
            while let Some(Ok(WsMessage::Binary(bytes))) = ws_receiver.next().await {
                let dispatcher = client.get_dispatcher();
                // We move the blocking lock() and synchronous read_bytes() call
                // onto a dedicated blocking thread to avoid freezing the test runtime.
                task::spawn_blocking(move || dispatcher.lock().unwrap().read_bytes(&bytes))
                    .await
                    .unwrap() // Unwrap JoinError
                    .unwrap(); // Unwrap Result from read_bytes
            }
        }
    });

    // 4. Make RPC calls using the WASM client instance.
    let (res1, res2, res3, res4, res5, res6) = join!(
        Add::call(client.as_ref(), vec![1.0, 2.0, 3.0]),
        Add::call(client.as_ref(), vec![8.0, 3.0, 7.0]),
        Mult::call(client.as_ref(), vec![8.0, 3.0, 7.0]),
        Mult::call(client.as_ref(), vec![1.5, 2.5, 8.5]),
        Echo::call(client.as_ref(), b"testing 1 2 3".to_vec()),
        Echo::call(client.as_ref(), b"testing 4 5 6".to_vec()),
    );

    // 5. Assert results.
    assert_eq!(res1.unwrap(), 6.0);
    assert_eq!(res2.unwrap(), 18.0);
    assert_eq!(res3.unwrap(), 168.0);
    assert_eq!(res4.unwrap(), 31.875);
    assert_eq!(res5.unwrap(), b"testing 1 2 3");
    assert_eq!(res6.unwrap(), b"testing 4 5 6");
}

#[tokio::test]
async fn test_error_client_server_roundtrip() {
    // 1. Start a real Tokio-based RPC Server.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("ws://{addr}/ws");

    // Use the same Arc/endpoint pattern for consistency.
    let server = Arc::new(RpcServer::new());
    let endpoint = server.endpoint();

    endpoint
        .register_prebuffered(Add::METHOD_ID, |_, _bytes| async move {
            Err("Addition failed".into())
        })
        .await
        .unwrap();

    tokio::spawn({
        let server = Arc::clone(&server);
        async move {
            let _ = server.serve_with_listener(listener).await;
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // 2. Create the WASM client and the WebSocket bridge.
    let (to_bridge_tx, mut to_bridge_rx) = tokio_mpsc::unbounded_channel::<Vec<u8>>();
    let client = Arc::new(RpcWasmClient::new(move |bytes| {
        to_bridge_tx.send(bytes).unwrap();
    }));

    let (ws_stream, _) = connect_async(&server_url).await.expect("Failed to connect");
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    tokio::spawn(async move {
        while let Some(bytes) = to_bridge_rx.recv().await {
            if ws_sender
                .send(WsMessage::Binary(bytes.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // This task is also updated to be non-blocking.
    tokio::spawn({
        let client = client.clone();
        async move {
            while let Some(Ok(WsMessage::Binary(bytes))) = ws_receiver.next().await {
                let dispatcher = client.get_dispatcher();
                task::spawn_blocking(move || dispatcher.lock().unwrap().read_bytes(&bytes))
                    .await
                    .unwrap()
                    .unwrap();
            }
        }
    });

    // 3. Make the failing RPC call.
    let res = Add::call(client.as_ref(), vec![1.0, 2.0, 3.0]).await;

    // 4. Assert that the error was propagated correctly.
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    assert!(
        err.to_string()
            .contains("Remote system error: Addition failed")
    );
}

#[tokio::test]
async fn test_large_prebuffered_payload_roundtrip_wasm() {
    // 1. --- SETUP: START A REAL RPC SERVER ---
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("ws://{addr}/ws");
    let server = Arc::new(RpcServer::new());
    let endpoint = server.endpoint();

    // Register a simple "echo" handler on the server for our test to call.
    endpoint
        .register_prebuffered(Echo::METHOD_ID, |_, bytes: Vec<u8>| async move {
            // The handler simply returns the bytes it received.
            let params = Echo::decode_request(&bytes)?;
            Ok(Echo::encode_response(params)?)
        })
        .await
        .unwrap();

    // Spawn the server to run in the background.
    tokio::spawn(async move {
        let _ = server.serve_with_listener(listener).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 2. --- SETUP: CREATE WASM CLIENT AND BRIDGE ---
    let (to_bridge_tx, mut to_bridge_rx) = tokio_mpsc::unbounded_channel::<Vec<u8>>();
    let client = Arc::new(RpcWasmClient::new(move |bytes| {
        to_bridge_tx.send(bytes).unwrap();
    }));
    let (ws_stream, _) = connect_async(&server_url)
        .await
        .expect("Failed to connect to server");
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Bridge from WasmClient to real WebSocket
    tokio::spawn(async move {
        while let Some(bytes) = to_bridge_rx.recv().await {
            if ws_sender
                .send(WsMessage::Binary(bytes.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Bridge from real WebSocket to WasmClient
    tokio::spawn({
        let client = client.clone();
        async move {
            while let Some(Ok(WsMessage::Binary(bytes))) = ws_receiver.next().await {
                let dispatcher = client.get_dispatcher();
                task::spawn_blocking(move || dispatcher.lock().unwrap().read_bytes(&bytes))
                    .await
                    .unwrap()
                    .unwrap();
            }
        }
    });

    // 3. --- TEST: SEND AND RECEIVE A LARGE PAYLOAD ---

    // Create a payload that is 200x the chunk size to ensure
    // hundreds of chunks are streamed for both request and response.
    let large_payload = vec![42u8; DEFAULT_SERVICE_MAX_CHUNK_SIZE * 200];

    // Use the high-level `Echo::call` which uses the RpcCallPrebuffered trait.
    // This is a full, end-to-end test of the prebuffered logic through the WASM client.
    let result = Echo::call(client.as_ref(), large_payload.clone()).await;

    // 4. --- ASSERT ---
    assert!(
        result.is_ok(),
        "The RPC call for a large payload failed: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), large_payload);
}
