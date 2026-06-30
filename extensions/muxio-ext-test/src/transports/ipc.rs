use crate::endpoint_helpers;
use crate::test_transport::TestTransport;
use async_trait::async_trait;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, ToNsName, tokio::prelude::*};
use muxio_core::rpc::rpc_internals::RpcStreamEvent;
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
use muxio_tokio_rpc_ipc_client::RpcIpcClient;
use muxio_tokio_rpc_ipc_server::{
    RpcIpcConnectionContextHandle, RpcIpcServer, RpcIpcServerEvent,
};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use tokio::time::{Duration, sleep};

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
        let socket_name = temp_name("roundtrip");
        let server = RpcIpcServer::new(None);
        let endpoint = server.endpoint();
        endpoint_helpers::register_standard_handlers(&*endpoint).await;
        endpoint_helpers::register_error_handler(&*endpoint).await;
        drop(endpoint);
        let name = socket_name.clone();
        tokio::spawn(async move {
            let _ = server.serve(&name).await;
        });
        sleep(Duration::from_millis(200)).await;
        let client = RpcIpcClient::new(&socket_name).await.unwrap();
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
        let client = RpcIpcClient::new(&socket_name).await.unwrap();
        (client, tx)
    }

    async fn connect_s2c() -> (
        Arc<Self::Client>,
        Arc<RpcServiceEndpoint<()>>,
        Self::S2cHandle,
    ) {
        let socket_name = temp_name("s2c");
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let server = RpcIpcServer::new(Some(event_tx));
        // Register Echo on the server endpoint so client-initiated calls work
        endpoint_helpers::register_echo_handler(&*server.endpoint()).await;
        let name = socket_name.clone();
        tokio::spawn(async move {
            let _ = server.serve(&name).await;
        });
        sleep(Duration::from_millis(200)).await;

        let client = RpcIpcClient::new(&socket_name).await.unwrap();
        let endpoint = client.get_endpoint();

        let ctx_handle = loop {
            if let Some(RpcIpcServerEvent::ClientConnected(handle)) = event_rx.recv().await {
                break handle;
            }
            sleep(Duration::from_millis(10)).await;
        };

        (client, endpoint, ctx_handle)
    }

    async fn connect_for_streaming() -> (Arc<Self::Client>, Arc<RpcServiceEndpoint<()>>, Arc<Mutex<Vec<RpcStreamEvent>>>) {
        let socket_name = temp_name("streaming");
        let server = RpcIpcServer::new(None);
        let endpoint = server.endpoint();

        // Register standard prebuffered handlers (Add, Mult, Echo, error test)
        endpoint_helpers::register_standard_handlers(&*endpoint).await;
        endpoint_helpers::register_error_handler(&*endpoint).await;

        // Register streaming capture handler with shared events buffer.
        // Chunks are buffered locally and only emitted as a single response
        // when End arrives, so the response is always properly framed
        // regardless of transport batching.
        let events: Arc<Mutex<Vec<RpcStreamEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        endpoint
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

        drop(endpoint);
        let name = socket_name.clone();
        tokio::spawn(async move {
            let _ = server.serve(&name).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let client = RpcIpcClient::new(&socket_name).await.unwrap();
        let client_endpoint = client.get_endpoint();
        (client, client_endpoint, events)
    }
}
