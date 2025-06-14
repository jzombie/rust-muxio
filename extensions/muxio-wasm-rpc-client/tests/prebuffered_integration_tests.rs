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
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, prebuffered::RpcCallPrebuffered};
use muxio_tokio_rpc_server::{RpcServer, RpcServiceEndpointInterface};
use muxio_wasm_rpc_client::RpcWasmClient;
use std::sync::Arc;
use tokio::join;
use tokio::net::TcpListener;
use tokio::sync::mpsc as tokio_mpsc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

#[tokio::test]
async fn test_success_client_server_roundtrip() {
    // 1. Start a real Tokio-based RPC Server (identical to the non-WASM test)
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("ws://{}/ws", addr);

    let server = RpcServer::new();

    // Register handlers on the server.
    let _ = join!(
        server.register_prebuffered(Add::METHOD_ID, |_, bytes| async move {
            let req = Add::decode_request(&bytes)?;
            let result = req.iter().sum();
            let resp = Add::encode_response(result)?;
            Ok(resp)
        }),
        server.register_prebuffered(Mult::METHOD_ID, |_, bytes| async move {
            let req = Mult::decode_request(&bytes)?;
            let result = req.iter().product();
            let resp = Mult::encode_response(result)?;
            Ok(resp)
        }),
        server.register_prebuffered(Echo::METHOD_ID, |_, bytes| async move {
            let req = Echo::decode_request(&bytes)?;
            let resp = Echo::encode_response(req)?;
            Ok(resp)
        })
    );

    // Spawn the server to run in the background.
    tokio::spawn({
        let server = Arc::new(server);
        async move {
            let _ = server.serve_with_listener(listener).await;
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // 2. Create the WASM client instance.
    // This channel will capture bytes emitted by the client.
    let (to_bridge_tx, mut to_bridge_rx) = tokio_mpsc::unbounded_channel::<Vec<u8>>();
    let client = Arc::new(RpcWasmClient::new(move |bytes| {
        to_bridge_tx.send(bytes).unwrap();
    }));

    // 3. Setup the WebSocket bridge to connect the WASM client to the real server.
    let (ws_stream, _) = connect_async(&server_url)
        .await
        .expect("Failed to connect to server");
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // This task forwards messages from our client's emit callback to the WebSocket server.
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

    // This task forwards messages from the WebSocket server to our client's dispatcher.
    tokio::spawn({
        let client = client.clone();
        async move {
            while let Some(Ok(WsMessage::Binary(bytes))) = ws_receiver.next().await {
                // Lock the dispatcher and feed it the incoming bytes.
                client
                    .get_dispatcher()
                    .lock()
                    .unwrap()
                    .read_bytes(&bytes)
                    .unwrap();
            }
        }
    });

    // 4. Make RPC calls using the WASM client instance.
    // The bridge ensures these calls are transparently sent to the real server.
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
    let server_url = format!("ws://{}/ws", addr);

    let server = RpcServer::new();
    // Register a handler that always returns an error.
    server
        .register_prebuffered(Add::METHOD_ID, |_, _bytes| async move {
            Err("Addition failed".into())
        })
        .await
        .unwrap();

    tokio::spawn({
        let server = Arc::new(server);
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

    tokio::spawn({
        let client = client.clone();
        async move {
            while let Some(Ok(WsMessage::Binary(bytes))) = ws_receiver.next().await {
                client
                    .get_dispatcher()
                    .lock()
                    .unwrap()
                    .read_bytes(&bytes)
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
