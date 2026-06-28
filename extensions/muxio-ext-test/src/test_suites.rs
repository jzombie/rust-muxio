use example_muxio_rpc_service_definition::prebuffered::{Add, Echo, Mult};
use muxio::rpc::RpcRequest;
use muxio_rpc_service::error::{RpcServiceError, RpcServiceErrorCode};
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::RpcServiceEndpointInterface;
use std::error::Error;
use std::sync::Arc;
use tokio::join;

/// Run the standard success roundtrip test (Add, Mult, Echo concurrent calls).
pub async fn roundtrip_success<C: RpcServiceCallerInterface>(client: &C) {
    let (res1, res2, res3, res4, res5, res6) = join!(
        Add::call(client, vec![1.0, 2.0, 3.0]),
        Add::call(client, vec![8.0, 3.0, 7.0]),
        Mult::call(client, vec![8.0, 3.0, 7.0]),
        Mult::call(client, vec![1.5, 2.5, 8.5]),
        Echo::call(client, b"testing 1 2 3".to_vec()),
        Echo::call(client, b"testing 4 5 6".to_vec()),
    );
    assert_eq!(res1.unwrap(), 6.0);
    assert_eq!(res2.unwrap(), 18.0);
    assert_eq!(res3.unwrap(), 168.0);
    assert_eq!(res4.unwrap(), 31.875);
    assert_eq!(res5.unwrap(), b"testing 1 2 3".to_vec());
    assert_eq!(res6.unwrap(), b"testing 4 5 6".to_vec());
}

/// Run the error roundtrip test — the transport already registered 0xBAD
/// with a failing handler. Just call it and assert error propagation.
pub async fn roundtrip_error<C: RpcServiceCallerInterface>(client: &C) {
    let request = RpcRequest {
        rpc_method_id: 0xBAD,
        rpc_param_bytes: Some(b"hello".to_vec()),
        rpc_prebuffered_payload_bytes: None,
        is_finalized: true,
    };
    let result = client
        .call_rpc_buffered(request, |bytes: &[u8]| bytes.to_vec())
        .await
        .unwrap();
    let (_encoder, inner) = result;
    let err = inner.unwrap_err();
    match err {
        RpcServiceError::Rpc(payload) => {
            assert!(
                payload.message.contains("test error"),
                "Expected 'test error' in '{}'",
                payload.message
            );
        }
        other => panic!("Expected RpcServiceError::Rpc, got {:?}", other),
    }
}

/// Run the large payload roundtrip test.
pub async fn roundtrip_large_payload<C: RpcServiceCallerInterface>(client: &C) {
    let large_payload = vec![42u8; 100_000];
    let result = Echo::call(client, large_payload.clone()).await;
    assert_eq!(result.unwrap(), large_payload);
}

/// Run the method-not-found test.
pub async fn roundtrip_method_not_found<C: RpcServiceCallerInterface>(client: &C) {
    let request = RpcRequest {
        rpc_method_id: 0xDEAD_BEEF,
        rpc_param_bytes: Some(b"hello".to_vec()),
        rpc_prebuffered_payload_bytes: None,
        is_finalized: true,
    };
    let result = client
        .call_rpc_buffered(request, |bytes: &[u8]| bytes.to_vec())
        .await
        .unwrap();
    let (_encoder, inner) = result;
    let err = inner.unwrap_err();
    match err {
        RpcServiceError::Rpc(payload) => {
            assert_eq!(
                payload.code,
                RpcServiceErrorCode::NotFound,
                "Expected NotFound, got {:?}",
                payload.code
            );
        }
        other => panic!("Expected RpcServiceError::Rpc, got {:?}", other),
    }
}

// ------------------------------------------------------------------
// Transport state test bodies
// ------------------------------------------------------------------

/// Assert that connecting to nothing yields a ConnectionRefused error.
pub async fn transport_connection_failure<F, Fut>(connect: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), std::io::Error>>,
{
    let result = connect().await;
    assert!(
        result.is_err(),
        "Expected connection failure, but connection succeeded"
    );
}

/// Assert the state change handler fires Connected then Disconnected.
pub async fn transport_state_change_handler<C, F, Fut>(connect: F)
where
    C: RpcServiceCallerInterface + 'static,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Arc<C>, std::io::Error>>,
{
    let client = connect().await.unwrap();
    let received_states = Arc::new(std::sync::Mutex::new(Vec::new()));
    let notify_disconnect = Arc::new(tokio::sync::Notify::new());

    let states = received_states.clone();
    let notify = notify_disconnect.clone();
    client
        .set_state_change_handler(move |state| {
            if state == RpcTransportState::Disconnected {
                notify.notify_one();
            }
            states.lock().unwrap().push(state);
        })
        .await;

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    drop(client);

    let notified = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        notify_disconnect.notified(),
    )
    .await;
    assert!(notified.is_ok(), "Timed out waiting for disconnect");

    let final_states = received_states.lock().unwrap();
    assert_eq!(
        *final_states,
        vec![
            RpcTransportState::Connected,
            RpcTransportState::Disconnected
        ],
        "Expected [Connected, Disconnected], got {:?}",
        *final_states
    );
}

// ------------------------------------------------------------------
// Server-to-client test body
// ------------------------------------------------------------------

/// Run a server-to-client Echo roundtrip.
/// The test registers an Echo handler on the client endpoint, then the
/// server calls Echo via the `ConnectionContextHandle` / `IpcConnectionContextHandle`
/// and asserts the response matches.
pub async fn server_to_client_echo<H, C, E>(
    _client: &C,
    client_endpoint: &impl RpcServiceEndpointInterface<E>,
    ctx_handle: &H,
    label: &str,
) where
    H: RpcServiceCallerInterface,
    C: RpcServiceCallerInterface,
    E: Send + Sync + Clone + 'static,
{
    client_endpoint
        .register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
            let request = Echo::decode_request(&request_bytes)?;
            Echo::encode_response(request).map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
        })
        .await
        .unwrap();

    let test_message = format!("hello from server to {} client", label).into_bytes();
    let result = Echo::call(ctx_handle, test_message.clone()).await;
    assert!(
        result.is_ok(),
        "Server-to-{label}-client Echo failed: {:?}",
        result.err()
    );
    assert_eq!(
        result.unwrap(),
        test_message,
        "{label} client echo mismatch"
    );
}

// ------------------------------------------------------------------
// TODO: Server-to-client streaming (disabled — `call_rpc_streaming`
// deadlocks for this direction because the readiness signal waits for
// a response header before returning the encoder, but the server can't
// send chunks (needs the encoder) and the client can't send a response
// (hasn't received the full request yet).  Once `call_rpc_streaming`
// supports server-initiated streaming, add the test body below and
// uncomment the `test_server_to_client_streaming_echo` test in the
// `server_to_client_tests` macro.
//
// ```rust,ignore
// pub async fn server_to_client_streaming_echo<H, C, E>(
//     _client: &C,
//     client_endpoint: &impl RpcServiceEndpointInterface<E>,
//     ctx_handle: &H,
//     label: &str,
// ) where
//     H: RpcServiceCallerInterface,
//     C: RpcServiceCallerInterface,
//     E: Send + Sync + Clone + 'static,
// {
//     client_endpoint
//         .register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
//             let request = Echo::decode_request(&request_bytes)?;
//             Echo::encode_response(request)
//                 .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
//         })
//         .await
//         .unwrap();
//
//     let payload =
//         format!("hello from server to {label} client via stream").into_bytes();
//
//     let request = RpcRequest {
//         rpc_method_id: Echo::METHOD_ID,
//         rpc_param_bytes: None,
//         rpc_prebuffered_payload_bytes: None,
//         is_finalized: false,
//     };
//     let (mut encoder, mut receiver) = ctx_handle
//         .call_rpc_streaming(request, DynamicChannelType::Unbounded)
//         .await
//         .expect("server-to-client streaming call failed");
//
//     for chunk in payload.chunks(16) {
//         encoder.write_bytes(chunk).expect("write_bytes failed");
//     }
//     encoder.flush().expect("flush failed");
//     encoder.end_stream().expect("end_stream failed");
//
//     let mut response = Vec::new();
//     while let Some(chunk) = receiver.next().await {
//         match chunk {
//             Ok(bytes) => response.extend_from_slice(&bytes),
//             Err(e) => panic!("{label} streaming response error: {e:?}"),
//         }
//     }
//
//     assert_eq!(response, payload, "{label} streaming echo mismatch");
// }
// ```

// ------------------------------------------------------------------
// Pending requests fail on disconnect
// ------------------------------------------------------------------

/// Assert that pending RPC calls fail when the transport disconnects.
pub async fn pending_requests_fail_on_disconnect<C, D, Dfut>(client: Arc<C>, trigger_disconnect: D)
where
    C: RpcServiceCallerInterface + 'static,
    D: FnOnce() -> Dfut,
    Dfut: std::future::Future<Output = ()>,
{
    let client_clone = client.clone();
    let (tx_result, rx_result) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = Echo::call(client_clone.as_ref(), b"will fail".to_vec()).await;
        let _ = tx_result.send(result);
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    trigger_disconnect().await;

    let result = tokio::time::timeout(std::time::Duration::from_secs(3), rx_result)
        .await
        .expect("Timed out waiting for RPC call to resolve")
        .expect("Oneshot channel dropped");

    assert!(
        result.is_err(),
        "Expected the pending RPC call to fail, but it succeeded."
    );
}
