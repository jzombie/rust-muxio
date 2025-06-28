use axum::{
    Router,
    extract::ws::{Message, WebSocketUpgrade},
    routing::get,
};
use muxio_tokio_rpc_client::RpcClient;
use muxio_tokio_rpc_server::utils::tcp_listener_to_host_port;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::{
    net::TcpListener,
    sync::oneshot,
    time::{Duration, timeout},
};

#[tokio::test]
async fn test_client_responds_to_ping_with_pong() {
    // 1. --- SETUP: A MOCK SERVER THAT SENDS A PING ---
    let (tx, rx) = oneshot::channel::<bool>();
    // Wrap the sender to make it shareable and cloneable for the handler.
    let shared_tx = Arc::new(Mutex::new(Some(tx)));
    let ping_payload = b"heartbeat-check".to_vec();

    let app = Router::new().route(
        "/ws",
        get({
            // Clone the Arc for the `get` handler.
            let shared_tx = shared_tx.clone();
            move |ws: WebSocketUpgrade| async move {
                ws.on_upgrade(move |mut socket| async move {
                    // Send a ping from server to client
                    socket
                        .send(Message::Ping(ping_payload.clone().into()))
                        .await
                        .unwrap();

                    // Wait for the client's pong response
                    if let Ok(Some(Ok(Message::Pong(pong_payload)))) =
                        timeout(Duration::from_secs(1), socket.recv()).await
                    {
                        // Take ownership of the sender to consume it.
                        if let Some(tx) = shared_tx.lock().unwrap().take() {
                            let _ = tx.send(pong_payload.as_ref() == ping_payload.as_slice());
                        }
                    } else {
                        // Fail the test if no pong is received or if there's an error.
                        if let Some(tx) = shared_tx.lock().unwrap().take() {
                            let _ = tx.send(false);
                        }
                    }
                })
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();

    let server_task = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    // 2. --- SETUP: CONNECT THE RPC CLIENT ---
    // We only need the client to establish a connection; its internal tasks
    // should handle the pong automatically.
    let _client = RpcClient::new(&server_host.to_string(), server_port)
        .await
        .unwrap();

    // 3. --- ASSERT ---
    // Wait for the server to confirm it received the correct pong.
    let pong_received_correctly = timeout(Duration::from_secs(2), rx)
        .await
        .expect("Test timed out waiting for server confirmation")
        .expect("Oneshot channel was dropped");

    assert!(
        pong_received_correctly,
        "Client did not respond with a matching Pong message"
    );

    // Clean up the server task
    server_task.abort();
}
