mod client;
mod server;
pub mod service_definition;
pub use client::RpcClient;
use muxio::rpc::optional_traits::{RpcRequestPrebuffered, RpcResponsePrebuffered};
pub use server::RpcServer;
pub use service_definition::{Add, Mult};
use std::io;

// TODO: Create Call trait
impl Add {
    pub async fn call(rpc_client: &RpcClient, numbers: Vec<f64>) -> Result<f64, io::Error> {
        let dispatcher = rpc_client.dispatcher.clone();
        let tx = rpc_client.tx.clone();

        let payload = Add::encode_request(numbers);
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

// TODO: Create Call trait
impl Mult {
    pub async fn call(rpc_client: &RpcClient, numbers: Vec<f64>) -> Result<f64, io::Error> {
        let dispatcher = rpc_client.dispatcher.clone();
        let tx = rpc_client.tx.clone();

        let payload = Mult::encode_request(numbers);
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
