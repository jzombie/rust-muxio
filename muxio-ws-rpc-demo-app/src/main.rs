use muxio_ws_rpc_demo_app::{
    RpcClient, RpcServer, add,
    service_definition::{Add, RpcApi},
};
use tokio::join;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    // Bind to a random available port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = RpcServer::new();
    server
        .register(Add::METHOD_ID, |bytes| {
            let req = Add::decode_request(bytes).unwrap();
            let result = req.numbers.iter().sum();
            Add::encode_response(result)
        })
        .await;

    // Spawn the server using the pre-bound listener
    let _server_task = tokio::spawn({
        let server = server;
        async move {
            let _ = server.serve_with_listener(listener).await;
        }
    });

    // Wait briefly for server to start
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Use the actual bound address for the client
    let rpc_client = RpcClient::new(&format!("ws://{}/ws", addr)).await;

    let (res1, res2) = join!(
        add(&rpc_client, vec![1.0, 2.0, 3.0]),
        add(&rpc_client, vec![8.0, 3.0, 7.0])
    );

    println!("Result from first add(): {:?}", res1);
    println!("Result from second add(): {:?}", res2);
}
