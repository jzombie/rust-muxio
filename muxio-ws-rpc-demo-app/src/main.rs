mod client;
mod server;
mod service_definition;
use bitcode;
use client::RpcClient;
use server::RpcServer;
use service_definition::{Add, RpcApi};

async fn add(rpc_client: &RpcClient, numbers: Vec<f64>) -> Result<f64, bitcode::Error> {
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

#[tokio::main]
async fn main() {
    tokio::spawn(RpcServer::init("127.0.0.1:3000"));

    // sleep(Duration::from_millis(300)).await;

    let rpc_client = RpcClient::new("ws://127.0.0.1:3000/ws").await;

    let result = add(&rpc_client, vec![1.0, 2.0, 3.0]).await;
    println!("Result from add(): {:?}", result);

    let result = add(&rpc_client, vec![8.0, 3.0, 7.0]).await;
    println!("Result from add(): {:?}", result);
}
