use muxio_tokio_ipc_client::IpcClient;
use muxio_tokio_ipc_server::{IpcServer, IpcServerEvent};
use std::sync::Arc;
use tokio::time::{Duration, sleep};
use crate::ws_helpers::register_standard_handlers;

pub fn temp_socket_name(name: &str) -> String {
    format!("muxio-ipc-test-{}", name)
}

pub async fn setup_ipc_server(test_name: &str) -> String {
    let name = temp_socket_name(test_name);
    let server = IpcServer::new(None);
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
) -> (tokio::sync::mpsc::UnboundedReceiver<IpcServerEvent>, String) {
    let name = temp_socket_name(test_name);
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let server = IpcServer::new(Some(event_tx));
    register_standard_handlers(&*server.endpoint()).await;
    let server_name = name.clone();
    tokio::spawn(async move {
        let _ = server.serve(&server_name).await;
    });
    sleep(Duration::from_millis(200)).await;
    (event_rx, name)
}

pub async fn connect_ipc_client(socket_path: &str) -> Arc<IpcClient> {
    IpcClient::new(socket_path).await.unwrap()
}
