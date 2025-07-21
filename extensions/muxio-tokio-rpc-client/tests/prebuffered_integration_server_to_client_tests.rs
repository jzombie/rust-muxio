//! This test specifically verifies server-initiated RPC calls to the Tokio-based client.
//!
//! It sets up a real `RpcServer` and connects a `RpcClient` (the Tokio one).
//! The server then triggers an `Echo` RPC call directed at the connected client,
//! and the test asserts that the Tokio client correctly handles it and sends a response.

use example_muxio_rpc_service_definition::prebuffered::Echo;
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_tokio_rpc_client::RpcClient;
use muxio_tokio_rpc_server::{
    RpcServer, RpcServerEvent, RpcServiceEndpointInterface, utils::tcp_listener_to_host_port,
};
use std::error::Error;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::time::{Duration, sleep};

#[tokio::test]
async fn test_server_to_tokio_client_echo_roundtrip() {
    // 1. --- SETUP: Start a real RPC Server ---
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();

    // Channels to allow the test to interact with the server's event loop
    let (event_tx, mut event_rx) = tokio_mpsc::unbounded_channel::<RpcServerEvent>();

    // Wrap server in an Arc
    let server = Arc::new(RpcServer::new(Some(event_tx))); // Pass the event_tx

    // The server's own endpoint for handling client-initiated calls
    let _server_endpoint = server.endpoint();

    // Spawn the server to run in the background.
    let _server_task = tokio::spawn({
        let server = Arc::clone(&server);
        async move {
            let _ = server.serve_with_listener(listener).await;
        }
    });

    sleep(Duration::from_millis(100)).await; // Give server a moment to start

    // 2. --- SETUP: Connect the Tokio RPC client ---
    let client = RpcClient::new(&server_host.to_string(), server_port)
        .await
        .unwrap();

    // Register the Echo method on the Tokio client's endpoint
    // This is crucial for the server-to-client call to work.
    let client_endpoint = client.get_endpoint();
    client_endpoint
        .register_prebuffered(Echo::METHOD_ID, |_, request_bytes| async move {
            let request = Echo::decode_request(&request_bytes)?;
            tracing::info!(
                "TOKIO CLIENT (Test): Received server-initiated echo request: '{}'",
                String::from_utf8_lossy(&request)
            );
            Echo::encode_response(request).map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
        })
        .await
        .expect("Failed to register Echo method on Tokio client endpoint");

    // 3. --- TRIGGER: Wait for client connection and have server make a call ---
    let ctx_handle = loop {
        if let Some(RpcServerEvent::ClientConnected(handle)) = event_rx.recv().await {
            tracing::info!("Server detected client connected.");
            break handle;
        }
        sleep(Duration::from_millis(10)).await;
    };

    let test_message = b"hello from server to Tokio client test!".to_vec();
    tracing::info!("SERVER (Test): Initiating Echo call to Tokio client...");

    let server_to_client_echo_result = Echo::call(&ctx_handle, test_message.clone()).await;

    // 4. --- ASSERT ---
    assert!(
        server_to_client_echo_result.is_ok(),
        "Server-initiated Echo call to Tokio client failed: {:?}",
        server_to_client_echo_result.err()
    );

    let response = server_to_client_echo_result.unwrap();
    assert_eq!(
        response, test_message,
        "Tokio client did not echo the correct message back to server"
    );

    tracing::info!("SERVER (Test): Successfully received echo response from Tokio client.");
}
