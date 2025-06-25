use example_muxio_rpc_service_definition::prebuffered::{Add, Echo, Mult};
use muxio_rpc_service::{
    constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE, prebuffered::RpcMethodPrebuffered,
};
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_tokio_rpc_client::RpcClient;
use muxio_tokio_rpc_server::{RpcServer, RpcServiceEndpointInterface};
use std::sync::Arc;
use tokio::join;
use tokio::net::TcpListener;

// TODO: Add test to check that client errors if it cannot connect
// TODO: Add tests for transport state change handling

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
        let rpc_client = RpcClient::new(&format!("ws://{}/ws", addr)).await.unwrap();

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
        let rpc_client = RpcClient::new(&format!("ws://{}/ws", addr)).await.unwrap();
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

#[tokio::test]
async fn test_large_prebuffered_payload_roundtrip() {
    // 1. --- SETUP: START A REAL RPC SERVER ---
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("ws://{}/ws", addr);
    let server = Arc::new(RpcServer::new());
    let endpoint = server.endpoint();

    // Register a simple "echo" handler on the server for our test to call.
    endpoint
        .register_prebuffered(Echo::METHOD_ID, |_, bytes: Vec<u8>| async move {
            // The handler simply returns the bytes it received.
            Ok(Echo::encode_response(bytes).unwrap())
        })
        .await
        .unwrap();

    // Spawn the server to run in the background.
    tokio::spawn(async move {
        let _ = server.serve_with_listener(listener).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 2. --- SETUP: CONNECT A REAL RPC CLIENT ---
    let client = RpcClient::new(&server_url).await.unwrap();

    // 3. --- TEST: SEND AND RECEIVE A LARGE PAYLOAD ---

    // Create a payload that is 200x the chunk size to ensure
    // hundreds of chunks are streamed for both request and response.
    // 200 * 64KB = 12.8 MB
    let large_payload = vec![1u8; DEFAULT_SERVICE_MAX_CHUNK_SIZE * 200];

    // Use the high-level `Echo::call` which uses the RpcCallPrebuffered trait.
    // This is a full, end-to-end test of the prebuffered logic.
    let result = Echo::call(&client, large_payload.clone()).await;

    // 4. --- ASSERT ---
    assert!(
        result.is_ok(),
        "The RPC call for a large payload failed: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), large_payload);
}
