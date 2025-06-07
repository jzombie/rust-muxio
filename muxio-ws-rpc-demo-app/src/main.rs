mod client;
mod server;
mod service_definition;
use client::RpcClient;
use server::RpcServer;
use service_definition::{AddRequestParams, AddResponseParams};

async fn add(rpc_client: &RpcClient, numbers: Vec<f64>) -> f64 {
    let dispatcher = rpc_client.dispatcher.clone();
    let tx = rpc_client.tx.clone();

    let payload = bitcode::encode(&AddRequestParams { numbers });
    let (_dispatcher, result) = RpcClient::call_rpc(
        dispatcher,
        tx,
        0x01,
        payload,
        |bytes| {
            let decoded: AddResponseParams = bitcode::decode(&bytes).unwrap();
            decoded.result
        },
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
    println!("Result from add(): {}", result);

    let result = add(&rpc_client, vec![8.0, 3.0, 7.0]).await;
    println!("Result from add(): {}", result);
}
