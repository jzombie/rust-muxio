pub mod service_definition;
use muxio_service_traits::{RpcClientInterface, RpcRequestPrebuffered, RpcResponsePrebuffered};
use muxio_tokio_rpc_client::call_prebuffered_rpc;
pub use service_definition::{Add, Mult};
use std::io;

/// Trait for types that represent callable prebuffered RPC methods.
///
/// This trait forms the final layer of abstraction, allowing downstream
/// users to write `T::call(&client, input)` without dealing with traits
/// or transport logic explicitly.
#[async_trait::async_trait]
pub trait RpcCallPrebuffered:
    RpcRequestPrebuffered + RpcResponsePrebuffered + Sized + Send + Sync
{
    async fn call<C: RpcClientInterface + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error>;
}

#[async_trait::async_trait]
impl RpcCallPrebuffered for Add {
    async fn call<C: RpcClientInterface + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error> {
        call_prebuffered_rpc::<Add, C>(rpc_client, input).await
    }
}

#[async_trait::async_trait]
impl RpcCallPrebuffered for Mult {
    async fn call<C: RpcClientInterface + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error> {
        call_prebuffered_rpc::<Mult, C>(rpc_client, input).await
    }
}
