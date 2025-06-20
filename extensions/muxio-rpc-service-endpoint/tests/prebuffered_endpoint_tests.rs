use muxio::rpc::{RpcDispatcher, RpcRequest, RpcResponse, rpc_internals::RpcStreamEvent};
use muxio_rpc_service::RpcResultStatus;
use muxio_rpc_service::constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE;
use muxio_rpc_service_endpoint::{
    RpcServiceEndpoint, RpcServiceEndpointInterface,
    error::{HandlerPayloadError, RpcServiceEndpointError},
};
use std::sync::{Arc, Mutex};

/// A helper that simulates a full client -> server -> client RPC roundtrip.
async fn perform_request_response_cycle(
    endpoint: &RpcServiceEndpoint<()>,
    method_id: u64,
    param_bytes: &[u8],
) -> RpcResponse {
    let request = RpcRequest {
        rpc_method_id: method_id,
        rpc_param_bytes: Some(param_bytes.to_vec()),
        rpc_prebuffered_payload_bytes: None,
        is_finalized: true,
    };
    perform_request_response_cycle_with_request(endpoint, request).await
}

/// This version accepts a pre-constructed RpcRequest.
async fn perform_request_response_cycle_with_request(
    endpoint: &RpcServiceEndpoint<()>,
    request: RpcRequest,
) -> RpcResponse {
    let server_bound_buffer = Arc::new(Mutex::new(Vec::new()));
    let client_bound_buffer = Arc::new(Mutex::new(Vec::new()));

    let mut client_dispatcher = RpcDispatcher::new();

    let client_on_emit = {
        let server_bound_buffer = server_bound_buffer.clone();
        move |chunk: &[u8]| {
            server_bound_buffer.lock().unwrap().extend_from_slice(chunk);
        }
    };

    client_dispatcher
        .call(
            request,
            1024, // Use a small chunk size to ensure chunking happens in tests
            client_on_emit,
            None::<Box<dyn FnMut(RpcStreamEvent) + Send>>,
            false,
        )
        .unwrap();

    let request_bytes_chunks = server_bound_buffer.lock().unwrap().clone();

    // Create the dispatcher for the server-side simulation ONCE, before the loop.
    let mut dispatcher = RpcDispatcher::new();

    // The dispatcher will chunk large payloads, so we feed the server in pieces.
    for chunk in request_bytes_chunks.chunks(512) {
        let endpoint_on_emit = {
            let client_bound_buffer = client_bound_buffer.clone();
            move |resp_chunk: &[u8]| {
                client_bound_buffer
                    .lock()
                    .unwrap()
                    .extend_from_slice(resp_chunk);
            }
        };

        // Use the SAME dispatcher instance to process all chunks of the request.
        endpoint
            .read_bytes(&mut dispatcher, (), chunk, endpoint_on_emit)
            .await
            .unwrap();
    }

    let response_bytes = client_bound_buffer.lock().unwrap().clone();
    client_get_finalized_response(&mut client_dispatcher, &response_bytes)
}

/// Helper to read response bytes into a client dispatcher and extract the RpcResponse.
fn client_get_finalized_response(
    client_dispatcher: &mut RpcDispatcher,
    response_bytes: &[u8],
) -> RpcResponse {
    let request_ids = client_dispatcher.read_bytes(response_bytes).unwrap();
    // This assertion will now pass.
    assert_eq!(request_ids.len(), 1, "Client should have one response");

    let response_as_request = client_dispatcher
        .delete_rpc_request(request_ids[0])
        .unwrap();

    let result_status = response_as_request
        .rpc_param_bytes
        .as_ref()
        .and_then(|b| b.first().copied());

    RpcResponse {
        rpc_request_id: request_ids[0],
        rpc_method_id: response_as_request.rpc_method_id,
        rpc_result_status: result_status,
        rpc_prebuffered_payload_bytes: response_as_request.rpc_prebuffered_payload_bytes,
        is_finalized: response_as_request.is_finalized,
    }
}

#[tokio::test]
async fn test_handler_registration() {
    let endpoint = RpcServiceEndpoint::<()>::new();
    let result1 = endpoint
        .register_prebuffered(101, |_, _: Vec<u8>| async { Ok(vec![]) })
        .await;
    assert!(result1.is_ok());
    let result2 = endpoint
        .register_prebuffered(101, |_, _: Vec<u8>| async { Ok(vec![]) })
        .await;
    assert!(matches!(result2, Err(RpcServiceEndpointError::Handler(_))));
}

#[tokio::test]
async fn test_read_bytes_success() {
    let endpoint = Arc::new(RpcServiceEndpoint::<()>::new());
    const METHOD_ID: u64 = 202;
    endpoint
        .register_prebuffered(METHOD_ID, |_, req_bytes: Vec<u8>| async move {
            let num = u32::from_le_bytes(req_bytes.try_into().unwrap());
            Ok((num * 2).to_le_bytes().to_vec())
        })
        .await
        .unwrap();
    let response = perform_request_response_cycle(&endpoint, METHOD_ID, &5u32.to_le_bytes()).await;

    let status = RpcResultStatus::try_from(response.rpc_result_status.unwrap()).unwrap();
    assert_eq!(status, RpcResultStatus::Success);
    assert_eq!(
        response.rpc_prebuffered_payload_bytes.as_deref(),
        Some(&10u32.to_le_bytes()[..])
    );
}

#[tokio::test]
async fn test_read_bytes_handler_system_error() {
    let endpoint = Arc::new(RpcServiceEndpoint::<()>::new());
    const METHOD_ID: u64 = 303;
    let error_message = "a specific internal error occurred";

    endpoint
        .register_prebuffered(METHOD_ID, move |_, _: Vec<u8>| async move {
            Err(error_message.into())
        })
        .await
        .unwrap();
    let response = perform_request_response_cycle(&endpoint, METHOD_ID, &[]).await;

    let status = RpcResultStatus::try_from(response.rpc_result_status.unwrap()).unwrap();
    assert_eq!(status, RpcResultStatus::SystemError);
    assert_eq!(
        response.rpc_prebuffered_payload_bytes.as_deref(),
        Some(error_message.as_bytes())
    );
}

#[tokio::test]
async fn test_read_bytes_handler_fail_payload() {
    let endpoint = Arc::new(RpcServiceEndpoint::<()>::new());
    const METHOD_ID: u64 = 304;
    let error_payload = b"INVALID_ARGUMENT".to_vec();

    endpoint
        .register_prebuffered(METHOD_ID, {
            let error_payload = error_payload.clone();
            move |_, _: Vec<u8>| {
                let error_payload = error_payload.clone();
                async move {
                    Err(Box::new(HandlerPayloadError(error_payload))
                        as Box<dyn std::error::Error + Send + Sync>)
                }
            }
        })
        .await
        .unwrap();
    let response = perform_request_response_cycle(&endpoint, METHOD_ID, &[]).await;

    let status = RpcResultStatus::try_from(response.rpc_result_status.unwrap()).unwrap();
    assert_eq!(status, RpcResultStatus::Fail);
    assert_eq!(
        response.rpc_prebuffered_payload_bytes.as_deref(),
        Some(&error_payload[..])
    );
}

#[tokio::test]
async fn test_read_bytes_method_not_found() {
    let endpoint = Arc::new(RpcServiceEndpoint::<()>::new());
    const UNREGISTERED_METHOD_ID: u64 = 404;
    let response = perform_request_response_cycle(&endpoint, UNREGISTERED_METHOD_ID, &[]).await;

    let status = RpcResultStatus::try_from(response.rpc_result_status.unwrap()).unwrap();
    assert_eq!(status, RpcResultStatus::MethodNotFound);
    assert!(response.rpc_prebuffered_payload_bytes.is_none());
}

#[tokio::test]
async fn test_large_payload_request_response_cycle() {
    let endpoint = Arc::new(RpcServiceEndpoint::<()>::new());
    const LARGE_PAYLOAD_METHOD_ID: u64 = 505;

    let large_payload = vec![0u8; DEFAULT_SERVICE_MAX_CHUNK_SIZE * 10];
    let expected_response_payload = {
        let mut resp = large_payload.clone();
        resp.extend_from_slice(b"_processed");
        resp
    };

    endpoint
        .register_prebuffered(LARGE_PAYLOAD_METHOD_ID, {
            let expected_response = expected_response_payload.clone();
            move |_, req_bytes: Vec<u8>| {
                let mut resp_bytes = req_bytes.clone();
                resp_bytes.extend_from_slice(b"_processed");
                assert_eq!(resp_bytes, expected_response);
                async move { Ok(resp_bytes) }
            }
        })
        .await
        .unwrap();

    let request_with_large_payload = RpcRequest {
        rpc_method_id: LARGE_PAYLOAD_METHOD_ID,
        rpc_param_bytes: None,
        rpc_prebuffered_payload_bytes: Some(large_payload.clone()),
        is_finalized: true,
    };

    let response =
        perform_request_response_cycle_with_request(&endpoint, request_with_large_payload).await;

    let status = RpcResultStatus::try_from(response.rpc_result_status.unwrap()).unwrap();
    assert_eq!(status, RpcResultStatus::Success);
    assert_eq!(
        response.rpc_prebuffered_payload_bytes.as_deref(),
        Some(expected_response_payload.as_slice())
    );
}
