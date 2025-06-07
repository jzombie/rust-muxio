mod client;
mod server;
pub mod service_definition;

pub use client::RpcClient;
use muxio::rpc::RpcDispatcher;
use muxio::rpc::optional_traits::{RpcRequestPrebuffered, RpcResponsePrebuffered};
pub use server::RpcServer;
pub use service_definition::{Add, Mult};
use std::io;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;

// TODO: Extract `GenericRpcClient` into a separate crate

/// Trait representing the minimal interface required for an RPC client.
/// This enables decoupling from a specific `RpcClient` implementation.
#[cfg(not(target_arch = "wasm32"))]
pub trait GenericRpcClient {
    fn dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>>;
    fn sender(&self) -> UnboundedSender<tokio_tungstenite::tungstenite::protocol::Message>;
}

// TODO: Alternate handling, etc.
// #[cfg(target_arch = "wasm32")]
// pub trait GenericRpcClient { /* wasm-based version */ }

impl GenericRpcClient for RpcClient {
    fn dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    fn sender(&self) -> UnboundedSender<tokio_tungstenite::tungstenite::protocol::Message> {
        self.tx.clone()
    }
}

/// Calls a prebuffered RPC method defined by the `RpcRequestPrebuffered` and
/// `RpcResponsePrebuffered` traits using a generic RPC client.
///
/// The output type must be `'static` to comply with async executor bounds.
pub async fn call_prebuffered_rpc<T, C>(
    rpc_client: &C,
    input: T::Input,
) -> Result<T::Output, io::Error>
where
    T: RpcRequestPrebuffered + RpcResponsePrebuffered + Send + Sync + 'static,
    T::Output: Send + 'static,
    C: GenericRpcClient + Send + Sync,
{
    let dispatcher = rpc_client.dispatcher();
    let tx = rpc_client.sender();

    let payload = T::encode_request(input);

    let (_dispatcher, result) = RpcClient::call_rpc(
        dispatcher,
        tx,
        <T as RpcRequestPrebuffered>::METHOD_ID,
        payload,
        T::decode_response,
        true,
    )
    .await;

    result
}

/// Trait for types that represent callable prebuffered RPC methods.
///
/// This trait delegates to the generic `call_prebuffered_rpc` function using
/// the associated `Input` and `Output` types.
#[async_trait::async_trait]
pub trait RpcCallPrebuffered:
    RpcRequestPrebuffered + RpcResponsePrebuffered + Sized + Send + Sync
{
    async fn call<C: GenericRpcClient + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error>;
}

#[async_trait::async_trait]
impl RpcCallPrebuffered for Add {
    async fn call<C: GenericRpcClient + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error> {
        call_prebuffered_rpc::<Add, C>(rpc_client, input).await
    }
}

#[async_trait::async_trait]
impl RpcCallPrebuffered for Mult {
    async fn call<C: GenericRpcClient + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error> {
        call_prebuffered_rpc::<Mult, C>(rpc_client, input).await
    }
}
