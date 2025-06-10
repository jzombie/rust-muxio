pub mod service_definition;
use muxio_rpc_service::{RpcCallPrebuffered, RpcClientInterface, call_prebuffered_rpc};
pub use service_definition::{Add, Mult};
use std::io;

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
