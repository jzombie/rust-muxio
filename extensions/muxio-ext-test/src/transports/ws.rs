use async_trait::async_trait;
use muxio_rpc_service_endpoint::RpcServiceEndpoint;
use muxio_tokio_rpc_client::RpcClient;
use muxio_tokio_rpc_server::{
    RpcServer, RpcServerEvent, RpcServiceEndpointInterface as _,
    ConnectionContextHandle,
    utils::{tcp_listener_to_host_port, bind_tcp_listener_on_random_port},
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::{Duration, sleep};
use crate::test_transport::TestTransport;
use crate::ws_helpers;

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
        ws_helpers::register_standard_handlers(&*server_endpoint).await;
        // Pre-register a test error handler for the roundtrip_error test
        let _ = server_endpoint.register_prebuffered(
            0xBAD,
            |_request_bytes, _ctx| async move {
                Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "test error",
                )) as Box<dyn std::error::Error + Send + Sync>)
            },
        ).await;
        let client = ws_helpers::connect_ws_client(&host, port).await;
        let endpoint = client.get_endpoint();
        (client, endpoint)
    }

    async fn connect_fail() -> Result<(), std::io::Error> {
        let (_listener, port) = bind_tcp_listener_on_random_port().await.unwrap();
        drop(_listener);
        RpcClient::new("127.0.0.1", port + 1).await.map(|_| ())
    }

    async fn connect_with_disconnect(
    ) -> (Arc<Self::Client>, oneshot::Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (host, port) = tcp_listener_to_host_port(&listener).unwrap();
        let (tx, rx) = oneshot::channel();

        tokio::spawn(async move {
            if let Ok((socket, _)) = listener.accept().await {
                let _ws = tokio_tungstenite::accept_async(socket).await;
                let _ = rx.await;
            }
        });

        sleep(Duration::from_millis(100)).await;
        let client = RpcClient::new(&host.to_string(), port).await.unwrap();
        (client, tx)
    }

    async fn connect_s2c(
    ) -> (
        Arc<Self::Client>,
        Arc<RpcServiceEndpoint<()>>,
        Self::S2cHandle,
    ) {
        let (_server, mut event_rx, host, port) =
            ws_helpers::setup_ws_server_with_events().await;
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
