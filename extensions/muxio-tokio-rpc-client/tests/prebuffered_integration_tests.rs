use example_muxio_rpc_service_definition::prebuffered::{Add, Echo, Mult};
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_tokio_rpc_client::RpcClient;
use muxio_tokio_rpc_server::{RpcServer, RpcServiceEndpointInterface};
use std::sync::Arc;
use tokio::join;
use tokio::net::TcpListener;

/// This integration test creates a full, in-memory client-server roundtrip,
/// directly replicating the logic from the example application.
#[tokio::test]
async fn test_success_client_server_roundtrip() {
    // Bind to a random available port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // This block sets up and spawns the server
    {
        // Wrap the server in an Arc to manage ownership correctly.
        let server = Arc::new(RpcServer::new());

        // Get a handle to the endpoint for registration.
        let endpoint = server.endpoint();

        // Register handlers on the endpoint, not the server.
        let _ = join!(
            endpoint.register_prebuffered(Add::METHOD_ID, |_, bytes: Vec<u8>| async move {
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
            // Clone the Arc to move into the task.
            let server = Arc::clone(&server);
            async move {
                let _ = server.serve_with_listener(listener).await;
            }
        });
    }

    // This block runs the client
    {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let rpc_client = RpcClient::new(&format!("ws://{}/ws", addr)).await;

        let (res1, res2, res3, res4, res5, res6) = join!(
            Add::call(&rpc_client, vec![1.0, 2.0, 3.0]),
            Add::call(&rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&rpc_client, vec![1.5, 2.5, 8.5]),
            Echo::call(&rpc_client, b"testing 1 2 3".into()),
            Echo::call(&rpc_client, b"testing 4 5 6".into()),
        );

        assert_eq!(res1.unwrap(), 6.0);
        assert_eq!(res2.unwrap(), 18.0);
        assert_eq!(res3.unwrap(), 168.0);
        assert_eq!(res4.unwrap(), 31.875);
        assert_eq!(res5.unwrap(), b"testing 1 2 3");
        assert_eq!(res6.unwrap(), b"testing 4 5 6");
    }
}

#[tokio::test]
async fn test_error_client_server_roundtrip() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // This block sets up and spawns the server
    {
        // Use the same correct setup pattern.
        let server = Arc::new(RpcServer::new());
        let endpoint = server.endpoint();

        // Note: The `join!` macro is not strictly necessary for a single future,
        // but we use it here to show the pattern is consistent.
        let _ = join!(
            endpoint.register_prebuffered(Add::METHOD_ID, |_, _bytes: Vec<u8>| async move {
                Err("Addition failed".into())
            }),
        );

        let _server_task = tokio::spawn({
            let server = Arc::clone(&server);
            async move {
                let _ = server.serve_with_listener(listener).await;
            }
        });
    }

    // This block runs the client
    {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let rpc_client = RpcClient::new(&format!("ws://{}/ws", addr)).await;
        let res = Add::call(&rpc_client, vec![1.0, 2.0, 3.0]).await;

        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert!(
            err.to_string()
                .contains("Remote system error: Addition failed")
        );
    }
}
