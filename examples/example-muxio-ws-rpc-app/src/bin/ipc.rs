use example_muxio_rpc_service_definition::{
    RpcMethodPrebuffered,
    prebuffered::{Add, Echo, Mult},
};
use muxio_tokio_ipc_client::{
    IpcClient, RpcCallPrebuffered, RpcServiceCallerInterface, RpcTransportState,
};
use muxio_tokio_ipc_server::{IpcServer, RpcServiceEndpointInterface};
use tokio::join;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    // Use process ID to avoid collisions between concurrent test invocations
    let socket_name = format!("muxio-ipc-example-{}", std::process::id());

    // This block sets up and spawns the server
    {
        let server = IpcServer::new(None);
        let endpoint = server.endpoint();

        // Register server methods on the endpoint
        let _ = join!(
            endpoint.register_prebuffered(
                Add::METHOD_ID,
                |request_bytes: Vec<u8>, _ctx| async move {
                    let request_params = Add::decode_request(&request_bytes)?;
                    let sum = request_params.iter().sum();
                    let response_bytes = Add::encode_response(sum)?;
                    Ok(response_bytes)
                }
            ),
            endpoint.register_prebuffered(
                Mult::METHOD_ID,
                |request_bytes: Vec<u8>, _ctx| async move {
                    let request_params = Mult::decode_request(&request_bytes)?;
                    let product = request_params.iter().product();
                    let response_bytes = Mult::encode_response(product)?;
                    Ok(response_bytes)
                }
            ),
            endpoint.register_prebuffered(
                Echo::METHOD_ID,
                |request_bytes: Vec<u8>, _ctx| async move {
                    let request_params = Echo::decode_request(&request_bytes)?;
                    let response_bytes = Echo::encode_response(request_params)?;
                    Ok(response_bytes)
                }
            )
        );

        // Spawn the server; it runs forever in the background
        let server_name = socket_name.clone();
        tokio::spawn(async move {
            let _ = server.serve(&server_name).await;
        });
    }

    // This block runs the client against the server
    {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let rpc_client = IpcClient::new(&socket_name).await.unwrap();

        rpc_client
            .set_state_change_handler(move |new_state: RpcTransportState| {
                tracing::info!("[Callback] Transport state changed to: {:?}", new_state);
            })
            .await;

        let (res1, res2, res3, res4, res5, res6) = join!(
            Add::call(&*rpc_client, vec![1.0, 2.0, 3.0]),
            Add::call(&*rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&*rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&*rpc_client, vec![1.5, 2.5, 8.5]),
            Echo::call(&*rpc_client, b"testing 1 2 3".into()),
            Echo::call(&*rpc_client, b"testing 4 5 6".into()),
        );

        assert_eq!(res1.unwrap(), 6.0);
        assert_eq!(res2.unwrap(), 18.0);
        assert_eq!(res3.unwrap(), 168.0);
        assert_eq!(res4.unwrap(), 31.875);
        assert_eq!(res5.unwrap(), b"testing 1 2 3");
        assert_eq!(res6.unwrap(), b"testing 4 5 6");
    }
}
