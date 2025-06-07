mod client;
mod server;
pub mod service_definition;
use bitcode;
pub use client::RpcClient;
pub use server::RpcServer;
pub use service_definition::{Add, RpcApi};

pub async fn add(rpc_client: &RpcClient, numbers: Vec<f64>) -> Result<f64, bitcode::Error> {
    let dispatcher = rpc_client.dispatcher.clone();
    let tx = rpc_client.tx.clone();

    let payload = Add::encode_request(numbers);
    let (_dispatcher, result) = RpcClient::call_rpc(
        dispatcher,
        tx,
        Add::METHOD_ID,
        payload,
        Add::decode_response,
        true,
    )
    .await;

    result
}
