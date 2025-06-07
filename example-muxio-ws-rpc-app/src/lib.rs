mod client;
mod server;
pub mod service_definition;
pub use client::RpcClient;
use muxio::rpc::optional_traits::{RpcRequestPrebuffered, RpcResponsePrebuffered};
pub use server::RpcServer;
pub use service_definition::{Add, Mult};
use std::io;

pub async fn call_prebuffered_rpc<T>(
    rpc_client: &RpcClient,
    input: T::Input,
) -> Result<T::Output, io::Error>
where
    T: RpcRequestPrebuffered + RpcResponsePrebuffered + Send + Sync + 'static,
    T::Output: Send + 'static,
{
    let dispatcher = rpc_client.dispatcher.clone();
    let tx = rpc_client.tx.clone();

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

#[async_trait::async_trait]
pub trait RpcCall: RpcRequestPrebuffered + RpcResponsePrebuffered + Sized + Send + Sync {
    async fn call(rpc_client: &RpcClient, input: Self::Input) -> Result<Self::Output, io::Error>;
}

#[async_trait::async_trait]
impl RpcCall for Add {
    async fn call(rpc_client: &RpcClient, input: Self::Input) -> Result<Self::Output, io::Error> {
        call_prebuffered_rpc::<Add>(rpc_client, input).await
    }
}

#[async_trait::async_trait]
impl RpcCall for Mult {
    async fn call(rpc_client: &RpcClient, input: Self::Input) -> Result<Self::Output, io::Error> {
        call_prebuffered_rpc::<Mult>(rpc_client, input).await
    }
}
