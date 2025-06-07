use muxio_ws_rpc_demo_app::{
    RpcClient, RpcServer, add,
    service_definition::{Add, RpcApi},
};
use tokio::join;

const SERVER_ADDRESS: &str = "127.0.0.1:3000";

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
        server.serve(SERVER_ADDRESS).await;
    });

    // Optional delay to ensure server is up before client connects
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let rpc_client = RpcClient::new(&format!("ws://{}/ws", SERVER_ADDRESS)).await;

    // Run requests concurrently
    // `join!` waits for both futures to complete.
    let (res1, res2) = join!(
        add(&rpc_client, vec![1.0, 2.0, 3.0]),
        add(&rpc_client, vec![8.0, 3.0, 7.0])
    );

    println!("Result from first add(): {:?}", res1);
    println!("Result from second add(): {:?}", res2);

    // Optionally wait for server task to end
    // let _ = server_task.await;
}
