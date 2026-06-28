use crate::endpoint_helpers;
use crate::test_transport::TestTransport;
use async_trait::async_trait;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, ToNsName, tokio::prelude::*};
use muxio_rpc_service_endpoint::RpcServiceEndpoint;
use muxio_tokio_ipc_client::IpcClient;
use muxio_tokio_ipc_server::{
    IpcConnectionContextHandle, IpcServer, IpcServerEvent, RpcServiceEndpointInterface,
};
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::time::{Duration, sleep};

use std::sync::atomic::{AtomicU64, Ordering};

static IPC_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_name(name: &str) -> String {
    let n = IPC_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("muxio-ipc-test-{name}-{n}")
}

#[async_trait]
impl TestTransport for IpcClient {
    type Client = IpcClient;
    type S2cHandle = IpcConnectionContextHandle;

    fn name() -> &'static str {
        "ipc"
    }

    async fn connect() -> (Arc<Self::Client>, Arc<RpcServiceEndpoint<()>>) {
        let socket_name = temp_name("roundtrip");
        let server = IpcServer::new(None);
        let endpoint = server.endpoint();
        endpoint_helpers::register_standard_handlers(&*endpoint).await;
        // Pre-register a test error handler for the roundtrip_error test
        let _ = endpoint
            .register_prebuffered(0xBAD, |_request_bytes, _ctx| async move {
                Err(Box::new(std::io::Error::other("test error"))
                    as Box<dyn std::error::Error + Send + Sync>)
            })
            .await;
        drop(endpoint);
        let name = socket_name.clone();
        tokio::spawn(async move {
            let _ = server.serve(&name).await;
        });
        sleep(Duration::from_millis(200)).await;
        let client = IpcClient::new(&socket_name).await.unwrap();
        let client_endpoint = client.get_endpoint();
        (client, client_endpoint)
    }

    async fn connect_fail() -> Result<(), std::io::Error> {
        IpcClient::new(&temp_name("conn-fail")).await.map(|_| ())
    }

    async fn connect_with_disconnect() -> (Arc<Self::Client>, oneshot::Sender<()>) {
        let socket_name = temp_name("disconnect");
        let (tx, rx) = oneshot::channel();

        let ns_name = socket_name
            .clone()
            .to_ns_name::<GenericNamespaced>()
            .unwrap();
        let listener = ListenerOptions::new()
            .name(ns_name)
            .try_overwrite(true)
            .create_tokio()
            .unwrap();

        tokio::spawn(async move {
            if let Ok(_conn) = listener.accept().await {
                let _ = rx.await;
            }
        });

        sleep(Duration::from_millis(100)).await;
        let client = IpcClient::new(&socket_name).await.unwrap();
        (client, tx)
    }

    async fn connect_s2c() -> (
        Arc<Self::Client>,
        Arc<RpcServiceEndpoint<()>>,
        Self::S2cHandle,
    ) {
        let socket_name = temp_name("s2c");
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let server = IpcServer::new(Some(event_tx));
        // Register Echo on the server endpoint so client-initiated calls work
        endpoint_helpers::register_echo_handler(&*server.endpoint()).await;
        let name = socket_name.clone();
        tokio::spawn(async move {
            let _ = server.serve(&name).await;
        });
        sleep(Duration::from_millis(200)).await;

        let client = IpcClient::new(&socket_name).await.unwrap();
        let endpoint = client.get_endpoint();

        let ctx_handle = loop {
            if let Some(IpcServerEvent::ClientConnected(handle)) = event_rx.recv().await {
                break handle;
            }
            sleep(Duration::from_millis(10)).await;
        };

        (client, endpoint, ctx_handle)
    }
}
