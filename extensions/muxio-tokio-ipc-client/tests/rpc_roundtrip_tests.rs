use example_muxio_rpc_service_definition::prebuffered::{Add, Echo, Mult};
use muxio::rpc::RpcRequest;
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service_caller::RpcServiceCallerInterface;
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_tokio_ipc_client::IpcClient;
use muxio_tokio_ipc_server::{IpcServer, RpcServiceEndpointInterface};
use std::sync::Arc;
use tokio::join;

fn temp_socket_name(name: &str) -> String {
    format!("muxio-ipc-test-{}", name)
}

async fn setup_server_and_client(test_name: &str) -> (Arc<IpcClient>, String) {
    let name = temp_socket_name(test_name);

    let server = IpcServer::new(None);
    let endpoint = server.endpoint();

    let _ = join!(
        endpoint.register_prebuffered(Add::METHOD_ID, |request_bytes, _ctx| async move {
            let request_params = Add::decode_request(&request_bytes)?;
            let sum = request_params.iter().sum();
            let response_bytes = Add::encode_response(sum)?;
            Ok(response_bytes)
        }),
        endpoint.register_prebuffered(Mult::METHOD_ID, |request_bytes, _ctx| async move {
            let request_params = Mult::decode_request(&request_bytes)?;
            let product = request_params.iter().product();
            let response_bytes = Mult::encode_response(product)?;
            Ok(response_bytes)
        }),
        endpoint.register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
            let request_params = Echo::decode_request(&request_bytes)?;
            let response_bytes = Echo::encode_response(request_params)?;
            Ok(response_bytes)
        })
    );

    let server_name = name.clone();
    tokio::spawn(async move {
        let _ = server.serve(&server_name).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let client = IpcClient::new(&name).await.unwrap();
    (client, name)
}

#[tokio::test]
async fn test_success_client_server_roundtrip() {
    let (client, _name) = setup_server_and_client("roundtrip-success").await;

    let (res1, res2, res3, res4, res5, res6) = join!(
        Add::call(&*client, vec![1.0, 2.0, 3.0]),
        Add::call(&*client, vec![8.0, 3.0, 7.0]),
        Mult::call(&*client, vec![8.0, 3.0, 7.0]),
        Mult::call(&*client, vec![1.5, 2.5, 8.5]),
        Echo::call(&*client, b"testing 1 2 3".to_vec()),
        Echo::call(&*client, b"testing 4 5 6".to_vec()),
    );

    assert_eq!(res1.unwrap(), 6.0);
    assert_eq!(res2.unwrap(), 18.0);
    assert_eq!(res3.unwrap(), 168.0);
    assert_eq!(res4.unwrap(), 31.875);
    assert_eq!(res5.unwrap(), b"testing 1 2 3".to_vec());
    assert_eq!(res6.unwrap(), b"testing 4 5 6".to_vec());

    drop(client);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}

#[tokio::test]
async fn test_large_prebuffered_payload_roundtrip() {
    let (client, _name) = setup_server_and_client("roundtrip-large").await;

    let large_payload = vec![42u8; 100_000];
    let result = Echo::call(&*client, large_payload.clone()).await;
    assert_eq!(result.unwrap(), large_payload);

    drop(client);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}

#[tokio::test]
async fn test_method_not_found_error() {
    let (client, _name) = setup_server_and_client("roundtrip-notfound").await;

    let request = RpcRequest {
        rpc_method_id: 0xDEAD_BEEF,
        rpc_param_bytes: Some(b"hello".to_vec()),
        rpc_prebuffered_payload_bytes: None,
        is_finalized: true,
    };

    let result = client
        .call_rpc_buffered(request, |bytes: &[u8]| bytes.to_vec())
        .await;

    match result {
        Err(err) => {
            let err_string = err.to_string();
            assert!(
                err_string.contains("NotFound") || err_string.contains("not found"),
                "Error should indicate method not found. Got: {}",
                err_string
            );
        }
        Ok((_encoder, Ok(_response))) => {
            panic!("Expected method-not-found error, but got success");
        }
        Ok((_encoder, Err(rpc_err))) => {
            let err_string = rpc_err.to_string();
            assert!(
                err_string.contains("NotFound") || err_string.contains("not found"),
                "Error should indicate method not found. Got: {}",
                err_string
            );
        }
    }

    drop(client);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}
