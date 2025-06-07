use muxio_ws_rpc_demo_app::{
    RpcClient, RpcServer, add,
    service_definition::{Add, RpcApi},
};
use tokio;

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
    let _server_task = tokio::spawn(async move {
        server.serve("127.0.0.1:3000").await;
    });

    // Optional delay to ensure server is up before client connects
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let rpc_client = RpcClient::new("ws://127.0.0.1:3000/ws").await;

    let result = add(&rpc_client, vec![1.0, 2.0, 3.0]).await;
    println!("Result from add(): {:?}", result);

    let result = add(&rpc_client, vec![8.0, 3.0, 7.0]).await;
    println!("Result from add(): {:?}", result);

    // Optionally wait for server task to end
    // let _ = server_task.await;
}
