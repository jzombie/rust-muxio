use example_muxio_rpc_service_definition::{
    RpcMethodPrebuffered,
    prebuffered::{Add, Echo, Mult},
};
use muxio_tokio_rpc_client::{
    RpcCallPrebuffered, RpcClient, RpcServiceCallerInterface, TransportState,
};
use muxio_tokio_rpc_server::{RpcServer, RpcServiceEndpointInterface};
use std::sync::Arc;
use tokio::join;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    // Bind to a random available port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // This block sets up and spawns the server
    {
        // Create the server and immediately wrap it in an Arc for sharing
        let server = Arc::new(RpcServer::new());

        //  Get a handle to the endpoint to register handlers.
        let endpoint = server.endpoint();

        // Register server methods on the endpoint.
        let _ = join!(
            endpoint.register_prebuffered(Add::METHOD_ID, |_, bytes: Vec<u8>| async move {
                // The `?` operator works here because we map the error at the end
                let params = Add::decode_request(&bytes)?;
                let sum = params.iter().sum();
                let response_bytes = Add::encode_response(sum)?;
                Ok(response_bytes)
            }),
            endpoint.register_prebuffered(Mult::METHOD_ID, |_, bytes: Vec<u8>| async move {
                let params = Mult::decode_request(&bytes)?;
                let product = params.iter().product();
                let response_bytes = Mult::encode_response(product)?;
                Ok(response_bytes)
            }),
            endpoint.register_prebuffered(Echo::METHOD_ID, |_, bytes: Vec<u8>| async move {
                let params = Echo::decode_request(&bytes)?;
                let response_bytes = Echo::encode_response(params)?;
                Ok(response_bytes)
            })
        );

        // Spawn the server using the pre-bound listener
        let _server_task = tokio::spawn({
            // Clone the Arc for the server task
            let server = Arc::clone(&server);
            async move {
                let _ = server.serve_with_listener(listener).await;
            }
        });
    }

    // This block runs the client against the server
    {
        // Wait briefly for server to start
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Use the actual bound address for the client
        let rpc_client = RpcClient::new(&format!("ws://{}/ws", addr)).await.unwrap();

        rpc_client.set_state_change_handler(move |new_state: TransportState| {
            // This code will run every time the connection state changes.
            tracing::info!("[Callback] Transport state changed to: {:?}", new_state);
        });

        // `join!` will await all responses before proceeding
        let (res1, res2, res3, res4, res5, res6) = join!(
            Add::call(&rpc_client, vec![1.0, 2.0, 3.0]),
            Add::call(&rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&rpc_client, vec![1.5, 2.5, 8.5]),
            Echo::call(&rpc_client, b"testing 1 2 3".into()),
            Echo::call(&rpc_client, b"testing 4 5 6".into()),
        );

        tracing::info!("Result from first add(): {:?}", res1);
        tracing::info!("Result from second add(): {:?}", res2);
        tracing::info!("Result from first mult(): {:?}", res3);
        tracing::info!("Result from second mult(): {:?}", res4);
        tracing::info!(
            "Result from first echo(): {:?}",
            String::from_utf8(res5.unwrap())
        );
        tracing::info!(
            "Result from second echo(): {:?}",
            String::from_utf8(res6.unwrap())
        );
    }
}
