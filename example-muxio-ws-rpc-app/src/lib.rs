mod client;
mod server;
pub mod service_definition;

pub use client::RpcClient;
use muxio::rpc::{
    RpcDispatcher,
    optional_traits::{RpcRequestPrebuffered, RpcResponsePrebuffered},
};
pub use server::RpcServer;
pub use service_definition::{Add, Mult};
use std::io;
mod rpc_transport;
pub use rpc_transport::RpcTransport;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

/// Native implementation of `RpcTransport` for `RpcClient`
#[async_trait::async_trait]
impl RpcTransport for RpcClient {
    type Dispatcher = RpcDispatcher<'static>;
    type Sender = UnboundedSender<WsMessage>;
    type Mutex<T: Send> = Mutex<T>;

    fn dispatcher(&self) -> Arc<Self::Mutex<Self::Dispatcher>> {
        self.dispatcher.clone()
    }

    fn sender(&self) -> Self::Sender {
        self.tx.clone()
    }

    /// Delegates the call to the actual `RpcClient::call_rpc` implementation.
    async fn call_rpc<T, F>(
        dispatcher: Arc<Self::Mutex<Self::Dispatcher>>,
        sender: Self::Sender,
        method_id: u64,
        payload: Vec<u8>,
        response_handler: F,
        is_finalized: bool,
    ) -> Result<T, io::Error>
    where
        T: Send + 'static,
        F: Fn(Vec<u8>) -> T + Send + Sync + 'static,
    {
        let (_dispatcher, result) = RpcClient::call_rpc(
            dispatcher,
            sender,
            method_id,
            payload,
            response_handler,
            is_finalized,
        )
        .await;

        Ok(result)
    }
}

// TODO: Refactor
/// Calls a prebuffered RPC method defined by the `RpcRequestPrebuffered` and
/// `RpcResponsePrebuffered` traits using a generic RPC transport.
///
/// This layer exists to decouple call-site logic from the encoding/decoding and
/// transport mechanics, allowing for easier composition and testability.
pub async fn call_prebuffered_rpc<T, C>(
    rpc_client: &C,
    input: T::Input,
) -> Result<T::Output, io::Error>
where
    T: RpcRequestPrebuffered + RpcResponsePrebuffered + Send + Sync + 'static,
    T::Output: Send + 'static,
    C: RpcTransport + Send + Sync,
    C::Dispatcher: Send,
{
    let dispatcher = rpc_client.dispatcher();
    let tx = rpc_client.sender();

    let transport_result = C::call_rpc(
        dispatcher,
        tx,
        <T as RpcRequestPrebuffered>::METHOD_ID,
        T::encode_request(input),
        T::decode_response,
        true,
    )
    .await?;

    // Error propagation is handled in two steps using two named variables:
    //
    // 1. `transport_result`: Result<Result<T::Output, io::Error>, io::Error>
    //    - This comes from the transport layer (e.g., socket communication).
    //    - The outer Result represents transport-level errors (e.g., channel closed).
    //
    // 2. `rpc_result`: T::Output
    //    - This unwraps the inner Result from `transport_result`.
    //    - If the remote RPC logic failed, this propagates that application-level error.
    let rpc_result = transport_result?;
    Ok(rpc_result)
}

/// Trait for types that represent callable prebuffered RPC methods.
///
/// This trait forms the final layer of abstraction, allowing downstream
/// users to write `T::call(&client, input)` without dealing with traits
/// or transport logic explicitly.
#[async_trait::async_trait]
pub trait RpcCallPrebuffered:
    RpcRequestPrebuffered + RpcResponsePrebuffered + Sized + Send + Sync
{
    async fn call<C: RpcTransport + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error>;
}

#[async_trait::async_trait]
impl RpcCallPrebuffered for Add {
    async fn call<C: RpcTransport + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error> {
        call_prebuffered_rpc::<Add, C>(rpc_client, input).await
    }
}

#[async_trait::async_trait]
impl RpcCallPrebuffered for Mult {
    async fn call<C: RpcTransport + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error> {
        call_prebuffered_rpc::<Mult, C>(rpc_client, input).await
    }
}
