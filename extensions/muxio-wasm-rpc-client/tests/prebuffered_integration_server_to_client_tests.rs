//! This test specifically verifies server-initiated RPC calls to the WASM client.
//!
//! It sets up a real `RpcServer` and connects an `RpcWasmClient` via a WebSocket bridge.
//! The server then triggers an `Echo` RPC call directed at the connected WASM client,
//! and the test asserts that the WASM client correctly handles it and sends a response.

use example_muxio_rpc_service_definition::prebuffered::Echo;
use futures_util::{SinkExt, StreamExt};
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, prebuffered::RpcCallPrebuffered};
use muxio_tokio_rpc_server::{RpcServer, RpcServerEvent, RpcServiceEndpointInterface};
use muxio_wasm_rpc_client::RpcWasmClient;
use std::error::Error;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::task;
use tokio::time::{Duration, sleep};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage}; // Needed for the Echo handler registration

#[tokio::test]
async fn test_server_to_wasm_client_echo_roundtrip() {
    // 1. --- SETUP: Start a real Tokio-based RPC Server ---
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("ws://{addr}/ws");

    // Channels to allow the test to interact with the server's event loop
    let (event_tx, mut event_rx) = tokio_mpsc::unbounded_channel::<RpcServerEvent>();

    // Wrap server in an Arc
    let server = Arc::new(RpcServer::new(Some(event_tx))); // Pass the event_tx

    // The server's own endpoint is not directly used for server-to-client calls here,
    // but it's part of the server setup.
    let _server_endpoint = server.endpoint();

    // Spawn the server to run in the background.
    let _server_task = tokio::spawn({
        let server = Arc::clone(&server);
        async move {
            let _ = server.serve_with_listener(listener).await;
        }
    });

    sleep(Duration::from_millis(100)).await; // Give server a moment to start

    // 2. --- SETUP: Create the WASM client instance and its WebSocket Bridge ---
    let (to_bridge_tx, mut to_bridge_rx) = tokio_mpsc::unbounded_channel::<Vec<u8>>();
    let client = Arc::new(RpcWasmClient::new(move |bytes| {
        // This is the WASM client's emit_callback, sending bytes to the bridge
        to_bridge_tx.send(bytes).unwrap();
    }));

    // Register the Echo method on the WASM client's endpoint
    // This is crucial for the server-to-client call to work.
    let client_endpoint = client.get_endpoint();
    client_endpoint
        .register_prebuffered(Echo::METHOD_ID, |_, request_bytes| async move {
            let request = Echo::decode_request(&request_bytes)?;
            tracing::info!(
                "WASM CLIENT (Test): Received server-initiated echo request: '{}'",
                String::from_utf8_lossy(&request)
            );
            Echo::encode_response(request).map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
        })
        .await
        .expect("Failed to register Echo method on WASM client endpoint");

    // Connect the WebSocket bridge
    let (ws_stream, _) = connect_async(&server_url)
        .await
        .expect("Failed to connect to server");
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Bridge from WasmClient (output callback) to real WebSocket (sender)
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

    // Bridge from real WebSocket (receiver) to WasmClient (input `read_bytes`)
    tokio::spawn({
        let client = client.clone();
        async move {
            while let Some(Ok(WsMessage::Binary(bytes))) = ws_receiver.next().await {
                // This is the critical part: using the client's dispatcher and endpoint correctly.
                // In a real WASM environment, `static_muxio_read_bytes_uint8` would handle this.
                // Here, we simulate that internal logic directly within the test bridge.
                let dispatcher_arc = client.get_dispatcher();
                let endpoint_arc = client.get_endpoint();
                let emit_fn = client.get_emit_fn();

                // `spawn_blocking` is used because `dispatcher_arc.lock().unwrap()` is a blocking call.
                // `block_on` inside `spawn_blocking` is used to execute the async endpoint.read_bytes
                // without blocking the main Tokio runtime thread (which `spawn_blocking` handles for us).
                task::spawn_blocking(move || {
                    let mut dispatcher_guard =
                        dispatcher_arc.lock().expect("Dispatcher mutex poisoned");
                    let result = tokio::runtime::Handle::current().block_on(async {
                        endpoint_arc
                            .read_bytes(
                                &mut dispatcher_guard,
                                (), // No context needed for the endpoint in this test
                                &bytes,
                                move |chunk: &[u8]| {
                                    emit_fn(chunk.to_vec()); // Use the client's original emit_fn to send responses
                                },
                            )
                            .await
                    });

                    if let Err(e) = result {
                        tracing::error!(
                            "WASM Client (Test Bridge): Error processing inbound RPC bytes: {:?}",
                            e
                        );
                    }
                    Ok::<(), std::io::Error>(()) // Return a dummy Result for spawn_blocking
                })
                .await
                .unwrap(); // Unwrap spawn_blocking result
            }
        }
    });

    // 3. --- TRIGGER: Wait for client connection and have server make a call ---
    let ctx_handle = loop {
        if let Some(RpcServerEvent::ClientConnected(handle)) = event_rx.recv().await {
            tracing::info!("Server detected client connected.");
            break handle;
        }
        sleep(Duration::from_millis(10)).await;
    };

    let test_message = b"hello from server via WASM client test!".to_vec();
    tracing::info!("SERVER (Test): Initiating Echo call to WASM client...");

    let server_to_client_echo_result = Echo::call(&ctx_handle, test_message.clone()).await;

    // 4. --- ASSERT ---
    assert!(
        server_to_client_echo_result.is_ok(),
        "Server-initiated Echo call to WASM client failed: {:?}",
        server_to_client_echo_result.err()
    );

    let response = server_to_client_echo_result.unwrap();
    assert_eq!(
        response, test_message,
        "WASM client did not echo the correct message back to server"
    );

    tracing::info!("SERVER (Test): Successfully received echo response from WASM client.");
}
