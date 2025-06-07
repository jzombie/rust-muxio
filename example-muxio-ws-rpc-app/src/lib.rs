mod client;
mod server;
pub mod service_definition;
pub use client::RpcClient;
use muxio::rpc::optional_traits::{RpcRequestPrebuffered, RpcResponsePrebuffered};
pub use server::RpcServer;
pub use service_definition::{Add, Mult};
use std::io;

#[async_trait::async_trait]
pub trait RpcCall: RpcRequestPrebuffered + RpcResponsePrebuffered + Sized + Send + Sync {
    async fn call(rpc_client: &RpcClient, input: Self::Input) -> Result<Self::Output, io::Error>;
}

#[async_trait::async_trait]
impl RpcCall for Add {
    async fn call(rpc_client: &RpcClient, input: Self::Input) -> Result<Self::Output, io::Error> {
        let dispatcher = rpc_client.dispatcher.clone();
        let tx = rpc_client.tx.clone();

        let payload = Add::encode_request(input);
        let (_dispatcher, result) = RpcClient::call_rpc(
            dispatcher,
            tx,
            <Add as RpcRequestPrebuffered>::METHOD_ID,
            payload,
            Add::decode_response,
            true,
        )
        .await;

        result
    }
}

#[async_trait::async_trait]
impl RpcCall for Mult {
    async fn call(rpc_client: &RpcClient, input: Self::Input) -> Result<Self::Output, io::Error> {
        let dispatcher = rpc_client.dispatcher.clone();
        let tx = rpc_client.tx.clone();

        let payload = Mult::encode_request(input);
        let (_dispatcher, result) = RpcClient::call_rpc(
            dispatcher,
            tx,
            <Mult as RpcRequestPrebuffered>::METHOD_ID,
            payload,
            Mult::decode_response,
            true,
        )
        .await;

        result
    }
}
