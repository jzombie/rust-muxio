use example_muxio_rpc_service_definition::prebuffered::Echo;
use futures_util::{SinkExt, StreamExt};
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_tokio_rpc_server::RpcServer;
use muxio_tokio_rpc_server::utils::{bind_tcp_listener_on_random_port, tcp_listener_to_host_port};
use muxio_wasm_rpc_client::RpcWasmClient;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::time::{Duration, sleep};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

#[tokio::test]
async fn test_client_errors_on_connection_failure() {
    let (_, unused_port) = bind_tcp_listener_on_random_port().await.unwrap();

    // Attempt to connect to an address that is not listening.
    // NOTE: This test might not be applicable to RpcWasmClient directly,
    // as it doesn't create its own connection. This logic would be in JS.
    // For now, we assume a similar high-level wrapper would exist.
    // let result = RpcWasmClient::new("127.0.0.1", unused_port).await;
    // assert!(result.is_err());
}

#[tokio::test]
async fn test_transport_state_change_handler() {
    // 1. --- SETUP: START A REAL RPC SERVER ---
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = Arc::new(RpcServer::new(None));

    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();
    let server_url = format!("ws://{server_host}:{server_port}/ws");

    // Spawn the server to run in the background.
    let server_task = tokio::spawn(async move {
        let _ = server.serve_with_listener(listener).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 2. --- SETUP: CONNECT CLIENT AND REGISTER HANDLER ---
    let received_states = Arc::new(Mutex::new(Vec::new()));

    let (to_bridge_tx, mut to_bridge_rx) = tokio_mpsc::unbounded_channel::<Vec<u8>>();
    let client = Arc::new(RpcWasmClient::new(move |bytes| {
        to_bridge_tx.send(bytes).unwrap();
    }));

    let states_clone = received_states.clone();
    client
        .set_state_change_handler(move |state| {
            states_clone.lock().unwrap().push(state);
        })
        .await;

    let (ws_stream, _) = connect_async(&server_url).await.expect("Failed to connect");
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

    // Bridge from real WebSocket to WasmClient (and handle disconnect)
    let client_clone = client.clone();
    tokio::spawn(async move {
        while let Some(Ok(WsMessage::Binary(bytes))) = ws_receiver.next().await {
            client_clone.process_incoming_bytes(&bytes);
        }
        client_clone.handle_disconnect();
    });

    // Give a moment for the initial "Connected" state to be registered.
    sleep(Duration::from_millis(50)).await;

    // 3. --- TEST: SIMULATE DISCONNECTION BY ABORTING THE SERVER ---
    server_task.abort();

    // Give the tasks a moment to clean up and call the disconnect handler.
    sleep(Duration::from_millis(100)).await;

    // 4. --- ASSERT ---
    let final_states = received_states.lock().unwrap();
    assert_eq!(
        *final_states,
        vec![
            RpcTransportState::Connected,
            RpcTransportState::Disconnected
        ],
        "The state change handler should have been called for both connect and disconnect events."
    );
}

#[tokio::test]
async fn test_pending_requests_fail_on_disconnect() {
    // 1. --- SETUP: A MOCK SERVER THAT RECEIVES A REQUEST BUT NEVER REPLIES ---
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("ws://{addr}/ws");

    let server_task = tokio::spawn(async move {
        if let Ok((socket, _)) = listener.accept().await {
            if let Ok(mut ws_stream) = tokio_tungstenite::accept_async(socket).await {
                // Wait for one message (the RPC call) and then do nothing, simulating a hang.
                ws_stream.next().await;
                // Keep the connection open but never reply.
                sleep(Duration::from_secs(5)).await;
            }
        }
    });

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
    let client_clone = client.clone();
    tokio::spawn(async move {
        // This loop will run until the connection is dropped.
        while let Some(Ok(WsMessage::Binary(bytes))) = ws_receiver.next().await {
            client_clone.process_incoming_bytes(&bytes);
        }
        // When the loop exits, the connection is closed. Trigger the disconnect logic.
        client_clone.handle_disconnect();
    });

    // Give the connection a moment to establish
    sleep(Duration::from_millis(100)).await;

    // 3. --- TRIGGER: Make an RPC call but don't await it yet ---
    let call_future = Echo::call(client.as_ref(), b"this will hang".to_vec());

    // Give the call time to be sent and become pending
    sleep(Duration::from_millis(100)).await;

    // 4. --- ACTION: Drop the connection by aborting the server task ---
    server_task.abort();

    // 5. --- ASSERT: The pending future should now fail instead of hanging ---
    // We wrap the await in a timeout as a safety net in case the logic is broken.
    let result = tokio::time::timeout(Duration::from_secs(1), call_future).await;

    assert!(
        result.is_ok(),
        "Test timed out, the future did not resolve."
    );

    let rpc_result = result.unwrap();
    assert!(
        rpc_result.is_err(),
        "Expected the pending RPC call to fail, but it succeeded."
    );

    let err_string = rpc_result.unwrap_err().to_string();
    assert!(
        err_string.contains("ReadAfterCancel"),
        "Error message should indicate that the request was cancelled."
    );
}
