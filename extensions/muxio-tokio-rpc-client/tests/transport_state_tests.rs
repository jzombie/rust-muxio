use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_tokio_rpc_client::RpcClient;
use muxio_tokio_rpc_server::RpcServer;
use std::sync::{Arc, Mutex};
use tokio::{
    net::TcpListener,
    time::{Duration, sleep},
};

#[tokio::test]
async fn test_client_errors_on_connection_failure() {
    // Attempt to connect to an address that is not listening.
    let invalid_address = "ws://127.0.0.1:1"; // Use a port that's almost certainly unused.
    let result = RpcClient::new(invalid_address).await;

    // Assert that the connection attempt resulted in an error.
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::ConnectionRefused);
}

#[tokio::test]
async fn test_transport_state_change_handler() {
    // 1. --- SETUP: START A REAL RPC SERVER ---
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("ws://{}/ws", addr);
    let server = Arc::new(RpcServer::new());

    // Spawn the server to run in the background.
    let _server_task = tokio::spawn(async move {
        let _ = server.serve_with_listener(listener).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 2. --- SETUP: CONNECT CLIENT AND REGISTER HANDLER ---
    let received_states = Arc::new(Mutex::new(Vec::new()));
    let client = RpcClient::new(&server_url).await.unwrap();

    let states_clone = received_states.clone();
    client.set_state_change_handler(move |state| {
        states_clone.lock().unwrap().push(state);
    });

    // Give a moment for the initial "Connected" state to be registered.
    sleep(Duration::from_millis(50)).await;

    // 3. --- TEST: SIMULATE DISCONNECTION BY DROPPING THE CLIENT ---
    // Dropping the client will run its Drop implementation, which aborts its
    // background tasks and reliably signals the disconnection.
    drop(client);

    // Give the tasks a moment to clean up and call the disconnect handler.
    sleep(Duration::from_millis(100)).await;

    // 4. --- ASSERT ---
    let final_states = received_states.lock().unwrap();
    assert_eq!(
        *final_states,
        vec![RpcTransportState::Connected, RpcTransportState::Disconnected],
        "The state change handler should have been called for both connect and disconnect events."
    );
}
