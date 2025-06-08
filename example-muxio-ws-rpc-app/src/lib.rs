mod client;
mod server;
pub mod service_definition;

pub use client::RpcClient;
use muxio::rpc::optional_traits::{RpcRequestPrebuffered, RpcResponsePrebuffered};
pub use server::RpcServer;
pub use service_definition::{Add, Mult};
use std::io;
mod rpc_transport;
pub use rpc_transport::RpcTransport;

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

    let result = C::call_rpc(
        dispatcher,
        tx,
        <T as RpcRequestPrebuffered>::METHOD_ID,
        T::encode_request(input),
        T::decode_response,
        true,
    )
    .await?;

    Ok(result?)
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
