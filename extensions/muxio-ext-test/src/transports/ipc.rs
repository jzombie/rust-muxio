use crate::test_transport::TestTransport;
use async_trait::async_trait;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, ToNsName, tokio::prelude::*};
use muxio_core::rpc::rpc_internals::RpcStreamEvent;

use muxio_rpc_service_endpoint::RpcServiceEndpoint;
use muxio_tokio_rpc_ipc_client::RpcIpcClient;
use muxio_tokio_rpc_ipc_server::{RpcIpcConnectionContextHandle, RpcIpcServerEvent};
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

use std::sync::atomic::{AtomicU64, Ordering};

static IPC_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_name(name: &str) -> String {
    let n = IPC_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("muxio-ipc-test-{name}-{n}")
}

#[async_trait]
impl TestTransport for RpcIpcClient {
    type Client = RpcIpcClient;
    type S2cHandle = RpcIpcConnectionContextHandle;

    fn name() -> &'static str {
        "ipc"
    }

    async fn connect() -> (Arc<Self::Client>, Arc<RpcServiceEndpoint<()>>) {
        let socket_name = crate::ipc_helpers::setup_ipc_server("roundtrip").await;
        let client = crate::ipc_helpers::connect_ipc_client(&socket_name).await;
        let client_endpoint = client.get_endpoint();
        (client, client_endpoint)
    }

    async fn connect_fail() -> Result<(), std::io::Error> {
        RpcIpcClient::new(&temp_name("conn-fail")).await.map(|_| ())
    }

    async fn connect_with_disconnect() -> (Arc<Self::Client>, oneshot::Sender<()>) {
        let socket_name = temp_name("disconnect");
        let (tx, rx) = oneshot::channel();

        let ns_name = socket_name
            .clone()
            .to_ns_name::<GenericNamespaced>()
            .unwrap();
        // Bound synchronously here, so the client below connects
        // deterministically — no readiness sleep required.
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

        let client = crate::ipc_helpers::connect_ipc_client(&socket_name).await;
        (client, tx)
    }

    async fn connect_s2c() -> (
        Arc<Self::Client>,
        Arc<RpcServiceEndpoint<()>>,
        Self::S2cHandle,
    ) {
        let (mut event_rx, socket_name) =
            crate::ipc_helpers::setup_ipc_server_with_events("s2c").await;

        let client = crate::ipc_helpers::connect_ipc_client(&socket_name).await;
        let endpoint = client.get_endpoint();

        // `recv()` already waits for the next event; no polling sleep needed.
        let ctx_handle = loop {
            if let Some(RpcIpcServerEvent::ClientConnected(handle)) = event_rx.recv().await {
                break handle;
            }
        };

        (client, endpoint, ctx_handle)
    }

    async fn connect_for_streaming() -> (
        Arc<Self::Client>,
        Arc<RpcServiceEndpoint<()>>,
        Arc<Mutex<Vec<RpcStreamEvent>>>,
    ) {
        let (socket_name, events) =
            crate::ipc_helpers::setup_ipc_streaming_server("streaming").await;
        let client = crate::ipc_helpers::connect_ipc_client(&socket_name).await;
        let client_endpoint = client.get_endpoint();
        (client, client_endpoint, events)
    }
}
