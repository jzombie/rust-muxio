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

// Helper to create the WasmClient and its bridge for tests
async fn setup_wasm_client_bridge(
    server_url: &str,
) -> (
    Arc<RpcWasmClient>,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
) {
    let (to_bridge_tx, mut to_bridge_rx) = tokio_mpsc::unbounded_channel::<Vec<u8>>();
    let client = Arc::new(RpcWasmClient::new(move |bytes| {
        to_bridge_tx.send(bytes).unwrap();
    }));

    let (ws_stream, _) = connect_async(server_url)
        .await
        .expect("Failed to connect to server");
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // After connecting, immediately notify the client.
    client.handle_connect();

    // Bridge from WasmClient to real WebSocket
    let sender_handle = tokio::spawn(async move {
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
    let receiver_handle = tokio::spawn({
        let client = client.clone();
        async move {
            while let Some(Ok(WsMessage::Binary(bytes))) = ws_receiver.next().await {
                client.process_incoming_bytes(&bytes);
            }
            // When the loop exits, the connection is closed.
            client.handle_disconnect();
        }
    });

    (client, sender_handle, receiver_handle)
}

#[tokio::test]
async fn test_transport_state_change_handler() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();
    let server_url = format!("ws://{server_host}:{server_port}/ws");
    let server = Arc::new(RpcServer::new(None));
    let server_task = tokio::spawn({
        let server = Arc::clone(&server);
        async move {
            let _ = server.serve_with_listener(listener).await;
        }
    });

    let received_states = Arc::new(Mutex::new(Vec::new()));
    let (client, ..) = setup_wasm_client_bridge(&server_url).await;

    let states_clone = received_states.clone();
    client
        .set_state_change_handler(move |state| {
            states_clone.lock().unwrap().push(state);
        })
        .await;

    sleep(Duration::from_millis(50)).await;

    // Simulate disconnection by aborting the server
    server_task.abort();
    sleep(Duration::from_millis(100)).await;

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
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("ws://{addr}/ws");
    let server_task = tokio::spawn(async move {
        if let Ok((socket, _)) = listener.accept().await {
            if let Ok(mut ws_stream) = tokio_tungstenite::accept_async(socket).await {
                ws_stream.next().await; // Wait for one message
                sleep(Duration::from_secs(5)).await; // Hang
            }
        }
    });

    let (client, ..) = setup_wasm_client_bridge(&server_url).await;
    sleep(Duration::from_millis(50)).await;

    let call_future = Echo::call(client.as_ref(), b"this will hang".to_vec());
    sleep(Duration::from_millis(50)).await;

    server_task.abort();

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
