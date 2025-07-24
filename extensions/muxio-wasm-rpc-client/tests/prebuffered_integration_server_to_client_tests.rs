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
use tokio::time::{Duration, sleep};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

#[tokio::test]
async fn test_server_to_wasm_client_echo_roundtrip() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("ws://{addr}/ws");

    let (event_tx, mut event_rx) = tokio_mpsc::unbounded_channel::<RpcServerEvent>();
    let server: Arc<RpcServer> = Arc::new(RpcServer::new(Some(event_tx)));
    let _server_endpoint = server.endpoint();

    let _server_task = tokio::spawn({
        let server = Arc::clone(&server);
        async move {
            let _ = server.serve_with_listener(listener).await;
        }
    });

    sleep(Duration::from_millis(100)).await;

    let (to_bridge_tx, mut to_bridge_rx) = tokio_mpsc::unbounded_channel::<Vec<u8>>();
    let client = Arc::new(RpcWasmClient::new(move |bytes| {
        to_bridge_tx.send(bytes).unwrap();
    }));

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

    let (ws_stream, _) = connect_async(&server_url)
        .await
        .expect("Failed to connect to server");
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

    // --- CRUCIAL CHANGE STARTS HERE ---
    tokio::spawn({
        let client = client.clone();
        async move {
            while let Some(Ok(WsMessage::Binary(bytes))) = ws_receiver.next().await {
                // Now, simply call the comprehensive `process_incoming_bytes` method
                // on the RpcWasmClient. This method must be updated in `rpc_wasm_client.rs`
                // to handle the full three-stage processing (read, process, respond).
                client.process_incoming_bytes(&bytes).await;
            }
        }
    });
    // --- CRUCIAL CHANGE ENDS HERE ---

    let ctx_handle = loop {
        if let Some(RpcServerEvent::ClientConnected(handle)) = event_rx.recv().await {
            println!("CLIENT CONNECTED!!!!!");

            tracing::info!("Server detected client connected.");
            break handle;
        }
        sleep(Duration::from_millis(10)).await;
    };

    let test_message = b"hello from server via WASM client test!".to_vec();
    tracing::info!("SERVER (Test): Initiating Echo call to WASM client...");

    let server_to_client_echo_result = Echo::call(&ctx_handle, test_message.clone()).await;

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
