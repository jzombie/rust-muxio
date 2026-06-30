use crate::endpoint_helpers;
use crate::test_transport::TestTransport;
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use muxio_core::rpc::rpc_internals::RpcStreamEvent;
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
use muxio_tokio_rpc_server::{ConnectionContextHandle, RpcServer, RpcServerEvent};
use muxio_wasm_rpc_client::RpcWasmClient;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::{Duration, sleep};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

#[async_trait]
impl TestTransport for RpcWasmClient {
    type Client = RpcWasmClient;
    type S2cHandle = ConnectionContextHandle;

    fn name() -> &'static str {
        "wasm"
    }

    async fn connect() -> (Arc<Self::Client>, Arc<RpcServiceEndpoint<()>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("ws://{addr}/ws");
        let server = Arc::new(RpcServer::new(None));
        let server_endpoint = server.endpoint();
        endpoint_helpers::register_standard_handlers(&*server_endpoint).await;
        endpoint_helpers::register_error_handler(&*server_endpoint).await;
        let server_clone = server.clone();
        tokio::spawn(async move {
            let _ = server_clone.serve_with_listener(listener).await;
        });
        sleep(Duration::from_millis(200)).await;

        let (client, _send, _recv) = setup_wasm_bridge(&server_url).await;
        let endpoint = client.get_endpoint();
        (client, endpoint)
    }

    async fn connect_fail() -> Result<(), std::io::Error> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let url = format!("ws://{addr}/ws");
        connect_async(&url)
            .await
            .map(|_| ())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::ConnectionRefused, e.to_string()))
    }

    async fn connect_with_disconnect() -> (Arc<Self::Client>, oneshot::Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("ws://{addr}/ws");

        tokio::spawn(async move {
            if let Ok((socket, _)) = listener.accept().await {
                let _ = tokio_tungstenite::accept_async(socket).await;
                futures_util::future::pending::<()>().await;
            }
        });

        sleep(Duration::from_millis(200)).await;
        let (client, _send_h, recv_h) = setup_wasm_bridge(&server_url).await;

        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let _ = rx.await;
            recv_h.abort();
        });

        (client, tx)
    }

    async fn connect_s2c() -> (
        Arc<Self::Client>,
        Arc<RpcServiceEndpoint<()>>,
        Self::S2cHandle,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("ws://{addr}/ws");
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let server = Arc::new(RpcServer::new(Some(event_tx)));
        // Register standard handlers on the server endpoint so client-initiated calls work
        endpoint_helpers::register_standard_handlers(&*server.endpoint()).await;
        let server_clone = server.clone();
        tokio::spawn(async move {
            let _ = server_clone.serve_with_listener(listener).await;
        });
        sleep(Duration::from_millis(200)).await;

        let (client, _send, _recv) = setup_wasm_bridge(&server_url).await;
        let endpoint = client.get_endpoint();

        let ctx_handle = loop {
            if let Some(RpcServerEvent::ClientConnected(handle)) = event_rx.recv().await {
                break handle;
            }
            sleep(Duration::from_millis(10)).await;
        };

        (client, endpoint, ctx_handle)
    }

    async fn connect_for_streaming() -> (Arc<Self::Client>, Arc<RpcServiceEndpoint<()>>, Arc<Mutex<Vec<RpcStreamEvent>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("ws://{addr}/ws");
        let server = Arc::new(RpcServer::new(None));
        let server_endpoint = server.endpoint();
        endpoint_helpers::register_standard_handlers(&*server_endpoint).await;
        endpoint_helpers::register_error_handler(&*server_endpoint).await;

        let events: Arc<Mutex<Vec<RpcStreamEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        server_endpoint
            .register_stream_handler(
                endpoint_helpers::STREAMING_CAPTURE_METHOD_ID,
                move |event, respond, _ctx| {
                    captured.lock().unwrap().push(event.clone());
                    match &event {
                        RpcStreamEvent::PayloadChunk { bytes, .. } => {
                            respond.respond(bytes.clone(), false);
                        }
                        RpcStreamEvent::End { .. } => {
                            respond.respond(Vec::new(), true);
                        }
                        _ => {}
                    }
                },
            )
            .await
            .expect("Failed to register streaming capture handler");

        let server_clone = server.clone();
        tokio::spawn(async move {
            let _ = server_clone.serve_with_listener(listener).await;
        });
        sleep(Duration::from_millis(200)).await;

        let (client, _send, _recv) = setup_wasm_bridge(&server_url).await;
        let endpoint = client.get_endpoint();
        (client, endpoint, events)
    }
}

async fn setup_wasm_bridge(
    server_url: &str,
) -> (
    Arc<RpcWasmClient>,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
) {
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
            client.handle_disconnect().await;
        }
    });

    (client, ws_send_handle, ws_recv_handle)
}
