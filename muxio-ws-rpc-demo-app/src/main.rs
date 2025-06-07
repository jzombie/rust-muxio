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
    let server = RpcServer::new();
    server
        .register(Add::METHOD_ID, |bytes| {
            let req = Add::decode_request(bytes).unwrap();
            let result = req.numbers.iter().sum();
            Add::encode_response(result)
        })
        .await;

    // Spawn server in the background
    let server_task = tokio::spawn(async move {
        server.serve("127.0.0.1:3000").await;
    });

    // Optional delay to ensure server is up before client connects
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let rpc_client = RpcClient::new("ws://127.0.0.1:3000/ws").await;

    let result = add(&rpc_client, vec![1.0, 2.0, 3.0]).await;
    println!("Result from add(): {:?}", result);

    let result = add(&rpc_client, vec![8.0, 3.0, 7.0]).await;
    println!("Result from add(): {:?}", result);

    // Optionally wait for server task if needed:
    // let _ = server_task.await;
}
