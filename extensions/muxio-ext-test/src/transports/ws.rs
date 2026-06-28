use crate::endpoint_helpers;
use crate::test_transport::TestTransport;
use crate::ws_helpers;
use async_trait::async_trait;
use muxio_rpc_service_endpoint::RpcServiceEndpoint;
use muxio_tokio_rpc_client::RpcClient;
use muxio_tokio_rpc_server::{
    ConnectionContextHandle, RpcServerEvent, RpcServiceEndpointInterface as _,
    utils::tcp_listener_to_host_port,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::{Duration, sleep};

#[async_trait]
impl TestTransport for RpcClient {
    type Client = RpcClient;
    type S2cHandle = ConnectionContextHandle;

    fn name() -> &'static str {
        "ws"
    }

    async fn connect() -> (Arc<Self::Client>, Arc<RpcServiceEndpoint<()>>) {
        let (server, host, port) = ws_helpers::setup_ws_server().await;
        // Register handlers on the SERVER endpoint (where requests are processed)
        let server_endpoint = server.endpoint();
        endpoint_helpers::register_standard_handlers(&*server_endpoint).await;
        // Pre-register a test error handler for the roundtrip_error test
        let _ = server_endpoint
            .register_prebuffered(0xBAD, |_request_bytes, _ctx| async move {
                Err(Box::new(std::io::Error::other("test error"))
                    as Box<dyn std::error::Error + Send + Sync>)
            })
            .await;
        let client = ws_helpers::connect_ws_client(&host, port).await;
        let endpoint = client.get_endpoint();
        (client, endpoint)
    }

    async fn connect_fail() -> Result<(), std::io::Error> {
        // Bind to a random port, drop it, then try connecting to it.
        // The OS should return ConnectionRefused since the port is no longer bound.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        RpcClient::new("127.0.0.1", port).await.map(|_| ())
    }

    async fn connect_with_disconnect() -> (Arc<Self::Client>, oneshot::Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (host, port) = tcp_listener_to_host_port(&listener).unwrap();
        let (tx, rx) = oneshot::channel();
        let mut rx = Some(rx);

        tokio::spawn(async move {
            if let Ok((socket, _)) = listener.accept().await
                && let Ok(ws_stream) = tokio_tungstenite::accept_async(socket).await
            {
                use futures_util::{SinkExt, StreamExt};
                let (mut ws_sender, mut ws_receiver) = ws_stream.split();
                // Drive the WebSocket I/O until disconnect signal
                loop {
                    tokio::select! {
                        biased;
                        _ = rx.as_mut().unwrap() => { break; }
                        msg = ws_receiver.next() => {
                            match msg {
                                Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(payload))) => {
                                    let _ = ws_sender.send(tokio_tungstenite::tungstenite::Message::Pong(payload)).await;
                                }
                                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => { break; }
                                _ => {}
                            }
                        }
                    }
                }
                let _ = ws_sender.close().await;
            }
        });

        sleep(Duration::from_millis(100)).await;
        let client = RpcClient::new(&host.to_string(), port).await.unwrap();
        (client, tx)
    }

    async fn connect_s2c() -> (
        Arc<Self::Client>,
        Arc<RpcServiceEndpoint<()>>,
        Self::S2cHandle,
    ) {
        let (server, mut event_rx, host, port) = ws_helpers::setup_ws_server_with_events().await;
        // Register Echo on the server endpoint so client-initiated calls work
        endpoint_helpers::register_echo_handler(&*server.endpoint()).await;
        let client = ws_helpers::connect_ws_client(&host, port).await;
        let endpoint = client.get_endpoint();

        let ctx_handle = loop {
            if let Some(RpcServerEvent::ClientConnected(handle)) = event_rx.recv().await {
                break handle;
            }
            sleep(Duration::from_millis(10)).await;
        };

        (client, endpoint, ctx_handle)
    }
}
