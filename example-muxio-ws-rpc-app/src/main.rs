use example_muxio_ws_rpc_app::{
    RpcCallPrebuffered, RpcClient, RpcServer,
    service_definition::{Add, Mult},
};
use muxio::rpc::optional_traits::{RpcRequestPrebuffered, RpcResponsePrebuffered};
use tokio::join;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    // Bind to a random available port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = RpcServer::new();

    // Register server method
    // Note: If not using `join!`, each `register` call must be awaited.
    join!(
        server.register(<Add as RpcRequestPrebuffered>::METHOD_ID, |bytes| {
            let req = Add::decode_request(bytes).unwrap();
            let result = req.iter().sum();
            Add::encode_response(result)
        }),
        server.register(<Mult as RpcRequestPrebuffered>::METHOD_ID, |bytes| {
            let req = Mult::decode_request(bytes).unwrap();
            let result = req.iter().product();
            Mult::encode_response(result)
        })
    );

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

    // `join!` will await all responses before proceeding
    let (res1, res2, res3) = join!(
        Add::call(&rpc_client, vec![1.0, 2.0, 3.0]),
        Add::call(&rpc_client, vec![8.0, 3.0, 7.0]),
        Mult::call(&rpc_client, vec![8.0, 3.0, 7.0])
    );

    println!("Result from first add(): {:?}", res1);
    println!("Result from second add(): {:?}", res2);
    println!("Result from first mult(): {:?}", res3);
}
