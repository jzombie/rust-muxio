use example_muxio_rpc_service_definition::prebuffered::Echo;
use futures_util::StreamExt; // FIX: Add this use statement
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_tokio_rpc_client::RpcClient;
use muxio_tokio_rpc_server::RpcServer;
use muxio_tokio_rpc_server::utils::{bind_tcp_listener_on_random_port, tcp_listener_to_host_port};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::time::{Duration, sleep};

#[tokio::test]
async fn test_client_errors_on_connection_failure() {
    let (_, unused_port) = bind_tcp_listener_on_random_port().await.unwrap();

    // Attempt to connect to an address that is not listening.
    let result = RpcClient::new("127.0.0.1", unused_port).await;

    // Assert that the connection attempt resulted in an error.
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::ConnectionRefused);
}

#[tokio::test]
async fn test_transport_state_change_handler() {
    // 1. --- SETUP: START A REAL RPC SERVER ---
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = Arc::new(RpcServer::new(None));

    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();

    // Spawn the server to run in the background.
    let _server_task = tokio::spawn(async move {
        let _ = server.serve_with_listener(listener).await;
    });

    sleep(Duration::from_millis(100)).await;

    // 2. --- SETUP: CONNECT CLIENT AND REGISTER HANDLER ---
    let received_states = Arc::new(Mutex::new(Vec::new()));
    let client = RpcClient::new(&server_host.to_string(), server_port)
        .await
        .unwrap();

    let states_clone = received_states.clone();
    client
        .set_state_change_handler(move |state| {
            states_clone.lock().unwrap().push(state);
        })
        .await;

    // Give a moment for the initial "Connected" state to be registered.
    sleep(Duration::from_millis(50)).await;

    // 3. --- TEST: SIMULATE DISCONNECTION BY DROPPING THE CLIENT ---
    drop(client);

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
    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();

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

    // 2. --- SETUP: CONNECT THE TOKIO CLIENT ---
    let client = RpcClient::new(&server_host.to_string(), server_port)
        .await
        .unwrap();
    sleep(Duration::from_millis(50)).await;

    // 3. --- TRIGGER: Make an RPC call but don't await it yet ---
    let call_future = Echo::call(client.as_ref(), b"this will hang".to_vec());

    // Give the call time to be sent and become pending
    sleep(Duration::from_millis(50)).await;

    // 4. --- ACTION: Drop the connection by aborting the server task ---
    server_task.abort();

    // Give a moment for the client's background task to detect the disconnect.
    sleep(Duration::from_millis(100)).await;

    // 5. --- ASSERT: The pending future should now fail instead of hanging ---
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
        err_string.contains("Connection dropped"), // Check for the disconnect error message
        "Error message should indicate that the request was cancelled due to a disconnect."
    );
}
