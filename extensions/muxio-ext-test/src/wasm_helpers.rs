use futures_util::{SinkExt, StreamExt};
use muxio_tokio_rpc_server::RpcServer;
use muxio_wasm_rpc_client::RpcWasmClient;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::{Duration, sleep};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

pub async fn setup_wasm_server() -> (Arc<RpcServer>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("ws://{addr}/ws");
    let server = Arc::new(RpcServer::new(None));
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.serve_with_listener(listener).await;
    });
    sleep(Duration::from_millis(200)).await;
    (server, server_url)
}

/// Sets up a WASM client connected to the given server URL via a bidirectional WebSocket bridge.
pub async fn setup_wasm_client_bridge(
    server_url: &str,
) -> (Arc<RpcWasmClient>, JoinHandle<()>, JoinHandle<()>) {
    let (to_bridge_tx, mut to_bridge_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let client = Arc::new(RpcWasmClient::new(move |bytes| {
        let _ = to_bridge_tx.send(bytes);
    }));

    let (ws_stream, _) = connect_async(server_url).await.unwrap();
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    client.handle_connect().await;

    let ws_send_handle = tokio::spawn(async move {
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

    let ws_recv_handle = tokio::spawn({
        let client = client.clone();
        async move {
            while let Some(Ok(WsMessage::Binary(bytes))) = ws_receiver.next().await {
                client.read_bytes(&bytes).await;
            }
        }
    });

    (client, ws_send_handle, ws_recv_handle)
}
