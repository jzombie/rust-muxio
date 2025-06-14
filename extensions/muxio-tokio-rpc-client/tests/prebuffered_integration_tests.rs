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

    {
        let server = RpcServer::new();

        // Register server method
        // Note: If not using `join!`, each `register` call must be awaited.
        let _ = join!(
            server.register_prebuffered(Add::METHOD_ID, |_, bytes| async move {
                let req = Add::decode_request(&bytes)?;
                let result = req.iter().sum();
                let resp = Add::encode_response(result)?;
                Ok(resp)
            }),
            server.register_prebuffered(Mult::METHOD_ID, |_, bytes| async move {
                let req = Mult::decode_request(&bytes)?;
                let result = req.iter().product();
                let resp = Mult::encode_response(result)?;
                Ok(resp)
            }),
            server.register_prebuffered(Echo::METHOD_ID, |_, bytes| async move {
                let req = Echo::decode_request(&bytes)?;
                let resp = Echo::encode_response(req)?;
                Ok(resp)
            })
        );

        // Spawn the server using the pre-bound listener
        let _server_task = tokio::spawn({
            let server = server;
            async move {
                let _ = Arc::new(server).serve_with_listener(listener).await;
            }
        });
    }

    {
        // Wait briefly for server to start
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Use the actual bound address for the client
        let rpc_client = RpcClient::new(&format!("ws://{}/ws", addr)).await;

        // `join!` will await all responses before proceeding
        let (res1, res2, res3, res4, res5, res6) = join!(
            Add::call(&rpc_client, vec![1.0, 2.0, 3.0]),
            Add::call(&rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&rpc_client, vec![8.0, 3.0, 7.0]),
            Mult::call(&rpc_client, vec![1.5, 2.5, 8.5]),
            Echo::call(&rpc_client, b"testing 1 2 3".into()),
            Echo::call(&rpc_client, b"testing 4 5 6".into()),
        );

        // Assert that all results are correct.
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
    // Bind to a random available port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    {
        let server = RpcServer::new();

        // Register server method
        // Note: If not using `join!`, each `register` call must be awaited.
        let _ = join!(
            server.register_prebuffered(Add::METHOD_ID, |_, _bytes| async move {
                Err("Addition failed".into())
            }),
        );

        // Spawn the server using the pre-bound listener
        let _server_task = tokio::spawn({
            let server = server;
            async move {
                let _ = Arc::new(server).serve_with_listener(listener).await;
            }
        });
    }

    {
        // Wait briefly for server to start
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Use the actual bound address for the client
        let rpc_client = RpcClient::new(&format!("ws://{}/ws", addr)).await;

        // `join!` will await all responses before proceeding
        let res = Add::call(&rpc_client, vec![1.0, 2.0, 3.0]).await;

        // 1. Assert that the result is indeed an error.
        assert!(res.is_err());

        // 2. Unwrap the error. This is now safe because we know it's an Err.
        let err = res.unwrap_err();

        // 3. Assert that the io::Error is the kind we expect.
        assert_eq!(err.kind(), std::io::ErrorKind::Other);

        // 4. Assert that the error message contains the specific string from the server.
        // The endpoint logic converts the generic handler error into a "Remote system error".
        assert!(
            err.to_string()
                .contains("Remote system error: Addition failed")
        );
    }
}
