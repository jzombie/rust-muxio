use muxio_tokio_rpc_client::RpcClient;
use muxio_tokio_rpc_server::{RpcServer, RpcServerEvent, utils::tcp_listener_to_host_port};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::time::{Duration, sleep};

pub async fn setup_ws_server() -> (Arc<RpcServer>, String, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();
    let server = Arc::new(RpcServer::new(None));
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.serve_with_listener(listener).await;
    });
    sleep(Duration::from_millis(200)).await;
    (server, server_host.to_string(), server_port)
}

pub async fn setup_ws_server_with_events() -> (
    Arc<RpcServer>,
    tokio::sync::mpsc::UnboundedReceiver<RpcServerEvent>,
    String,
    u16,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let server = Arc::new(RpcServer::new(Some(event_tx)));
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.serve_with_listener(listener).await;
    });
    sleep(Duration::from_millis(200)).await;
    (server, event_rx, server_host.to_string(), server_port)
}

pub async fn connect_ws_client(host: &str, port: u16) -> Arc<RpcClient> {
    RpcClient::new(host, port).await.unwrap()
}
