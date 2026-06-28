use crate::endpoint_helpers::register_standard_handlers;
use muxio_tokio_rpc_ipc_client::RpcIpcClient;
use muxio_tokio_rpc_ipc_server::{RpcIpcServer, RpcIpcServerEvent};
use std::sync::Arc;
use tokio::time::{Duration, sleep};

pub fn temp_socket_name(name: &str) -> String {
    format!("muxio-ipc-test-{}", name)
}

pub async fn setup_ipc_server(test_name: &str) -> String {
    let name = temp_socket_name(test_name);
    let server = RpcIpcServer::new(None);
    register_standard_handlers(&*server.endpoint()).await;
    let server_name = name.clone();
    tokio::spawn(async move {
        let _ = server.serve(&server_name).await;
    });
    sleep(Duration::from_millis(200)).await;
    name
}

pub async fn setup_ipc_server_with_events(
    test_name: &str,
) -> (tokio::sync::mpsc::UnboundedReceiver<RpcIpcServerEvent>, String) {
    let name = temp_socket_name(test_name);
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let server = RpcIpcServer::new(Some(event_tx));
    register_standard_handlers(&*server.endpoint()).await;
    let server_name = name.clone();
    tokio::spawn(async move {
        let _ = server.serve(&server_name).await;
    });
    sleep(Duration::from_millis(200)).await;
    (event_rx, name)
}

pub async fn connect_ipc_client(socket_path: &str) -> Arc<RpcIpcClient> {
    RpcIpcClient::new(socket_path).await.unwrap()
}
