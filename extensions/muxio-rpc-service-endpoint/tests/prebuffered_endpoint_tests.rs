use muxio::rpc::{RpcDispatcher, RpcRequest, rpc_internals::RpcStreamEvent};
use muxio_rpc_service::RpcResultStatus;
use muxio_rpc_service_endpoint::{
    RpcServiceEndpoint, RpcServiceEndpointInterface, error::RpcServiceEndpointError,
};
use std::sync::{Arc, Mutex};
use tokio;

/// A helper that simulates a full client -> server -> client RPC roundtrip.
/// It returns the final processed RpcRequest (which represents the response) from the
/// client dispatcher's internal queue.
async fn perform_request_response_cycle(
    endpoint: &RpcServiceEndpoint<()>,
    method_id: u64,
    param_bytes: &[u8],
) -> RpcRequest {
    // 1. Simulate the network transport between client and server.
    let server_bound_buffer = Arc::new(Mutex::new(Vec::new())); // Client -> Server
    let client_bound_buffer = Arc::new(Mutex::new(Vec::new())); // Server -> Client

    // 2. Setup the client dispatcher to send the request.
    let mut client_dispatcher = RpcDispatcher::new();
    let request = RpcRequest {
        rpc_method_id: method_id,
        rpc_param_bytes: Some(param_bytes.to_vec()),
        rpc_prebuffered_payload_bytes: None,
        is_finalized: true,
    };

    // The client's on_emit sends data to the server's buffer.
    let client_on_emit = {
        let server_bound_buffer = server_bound_buffer.clone();
        move |chunk: &[u8]| {
            server_bound_buffer.lock().unwrap().extend_from_slice(chunk);
        }
    };

    // This initiates the call and registers a response handler inside the dispatcher.
    client_dispatcher
        .call(
            request,
            1024,
            client_on_emit,
            None::<Box<dyn FnMut(RpcStreamEvent) + Send>>,
            false,
        )
        .unwrap();

    // 3. The endpoint reads the request from the server buffer.
    //    Its on_emit sends the response to the client's buffer.
    let endpoint_on_emit = {
        let client_bound_buffer = client_bound_buffer.clone();
        move |chunk: &[u8]| {
            client_bound_buffer.lock().unwrap().extend_from_slice(chunk);
        }
    };
    endpoint
        .read_bytes((), &server_bound_buffer.lock().unwrap(), endpoint_on_emit)
        .await
        .unwrap();

    // 4. The client dispatcher reads the response from the client buffer.
    //    This populates its internal response queue.
    let request_ids = client_dispatcher
        .read_bytes(&client_bound_buffer.lock().unwrap())
        .unwrap();
    assert_eq!(request_ids.len(), 1, "Client should have one response");

    // 5. Retrieve the processed response from the client's queue for verification.
    client_dispatcher
        .delete_rpc_request(request_ids[0])
        .unwrap()
}

#[tokio::test]
async fn test_handler_registration() {
    let endpoint = RpcServiceEndpoint::<()>::new();
    let result1 = endpoint
        .register_prebuffered(101, |_, _| async { Ok(vec![]) })
        .await;
    assert!(result1.is_ok());
    let result2 = endpoint
        .register_prebuffered(101, |_, _| async { Ok(vec![]) })
        .await;
    assert!(matches!(result2, Err(RpcServiceEndpointError::Handler(_))));
}

#[tokio::test]
async fn test_read_bytes_success() {
    let endpoint = Arc::new(RpcServiceEndpoint::<()>::new());
    const METHOD_ID: u64 = 202;

    endpoint
        .register_prebuffered(METHOD_ID, |_, req_bytes| async move {
            let num = u32::from_le_bytes(req_bytes.try_into().unwrap());
            Ok((num * 2).to_le_bytes().to_vec())
        })
        .await
        .unwrap();

    let response = perform_request_response_cycle(&endpoint, METHOD_ID, &5u32.to_le_bytes()).await;

    // In the dispatcher's logic, the response status is stored in the metadata, which becomes the `rpc_param_bytes`.
    let status_byte = response.rpc_param_bytes.as_ref().unwrap()[0];
    let status = RpcResultStatus::try_from(status_byte).unwrap();

    assert_eq!(status, RpcResultStatus::Success);
    assert_eq!(
        response.rpc_prebuffered_payload_bytes.as_deref(),
        Some(&10u32.to_le_bytes()[..])
    );
}

#[tokio::test]
async fn test_read_bytes_handler_error_payload() {
    let endpoint = Arc::new(RpcServiceEndpoint::<()>::new());
    const METHOD_ID: u64 = 303;
    let error_message = "a specific error occurred";

    // Register a handler that returns a generic error.
    endpoint
        .register_prebuffered(
            METHOD_ID,
            move |_, _| async move { Err(error_message.into()) },
        )
        .await
        .unwrap();

    let response = perform_request_response_cycle(&endpoint, METHOD_ID, &[]).await;

    let status_byte = response.rpc_param_bytes.as_ref().unwrap()[0];
    let status = RpcResultStatus::try_from(status_byte).unwrap();

    // The endpoint logic should convert this into a SystemError.
    assert_eq!(status, RpcResultStatus::SystemError);

    // The payload should be the string representation of the error.
    assert_eq!(
        response.rpc_prebuffered_payload_bytes.as_deref(),
        Some(error_message.as_bytes())
    );
}

#[tokio::test]
async fn test_read_bytes_method_not_found() {
    let endpoint = Arc::new(RpcServiceEndpoint::<()>::new());
    const UNREGISTERED_METHOD_ID: u64 = 404;

    let response = perform_request_response_cycle(&endpoint, UNREGISTERED_METHOD_ID, &[]).await;

    let status_byte = response.rpc_param_bytes.as_ref().unwrap()[0];
    let status = RpcResultStatus::try_from(status_byte).unwrap();
    assert_eq!(status, RpcResultStatus::MethodNotFound);
    assert!(response.rpc_prebuffered_payload_bytes.is_none());
}
