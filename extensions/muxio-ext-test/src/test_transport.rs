use async_trait::async_trait;
use muxio_core::rpc::rpc_internals::RpcStreamEvent;
use muxio_rpc_service_caller::RpcServiceCallerInterface;
use muxio_rpc_service_endpoint::RpcServiceEndpoint;
use std::sync::{Arc, Mutex};

/// Implement this for each transport to get all integration tests for free.
#[async_trait]
pub trait TestTransport: Sized {
    type Client: RpcServiceCallerInterface;
    type S2cHandle: RpcServiceCallerInterface + Send + 'static;

    fn name() -> &'static str;

    /// Start server, connect client, register standard handlers (Add, Mult, Echo)
    /// on the endpoint that processes RPC requests.
    /// Returns client and client endpoint.
    async fn connect() -> (Arc<Self::Client>, Arc<RpcServiceEndpoint<()>>);

    async fn connect_fail() -> Result<(), std::io::Error>;
    async fn connect_with_disconnect() -> (Arc<Self::Client>, tokio::sync::oneshot::Sender<()>);
    async fn connect_s2c() -> (
        Arc<Self::Client>,
        Arc<RpcServiceEndpoint<()>>,
        Self::S2cHandle,
    );

    /// Like connect() but also registers a streaming capture handler
    /// (at `rpc_method_id!("streaming.capture")`) on the server endpoint.
    /// Returns (client, client_endpoint, captured_events) where captured_events
    /// contains every RpcStreamEvent received by the streaming handler.
    async fn connect_for_streaming() -> (
        Arc<Self::Client>,
        Arc<RpcServiceEndpoint<()>>,
        Arc<Mutex<Vec<RpcStreamEvent>>>,
    );
}
