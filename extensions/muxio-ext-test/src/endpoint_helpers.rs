use example_muxio_rpc_service_definition::prebuffered::{Add, Echo, Mult};
use muxio_core::rpc::rpc_internals::RpcStreamEvent;
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service::rpc_method_id;
use muxio_rpc_service_endpoint::RpcServiceEndpointInterface;
use std::error::Error;
use std::sync::{Arc, Mutex};

/// Stable, collision-free method ID used for streaming handler capture tests.
/// Generated via `rpc_method_id!` hash, same mechanism as every real service definition.
pub const STREAMING_CAPTURE_METHOD_ID: u64 = rpc_method_id!("streaming.capture");

/// Shared method ID for error-handler tests across all transports.
pub const ERROR_TEST_METHOD_ID: u64 = rpc_method_id!("__test.error");

/// Method ID guaranteed to have no registered handler — used for method-not-found tests.
/// Uses a long, distinctive name that no real service would use.
pub const UNREGISTERED_METHOD_ID: u64 = rpc_method_id!("__test.unregistered.method");

/// Register a prebuffered error handler at `ERROR_TEST_METHOD_ID` (0xBAD).
pub async fn register_error_handler<C>(endpoint: &impl RpcServiceEndpointInterface<C>)
where
    C: Send + Sync + Clone + 'static,
{
    let _ = endpoint
        .register_prebuffered(ERROR_TEST_METHOD_ID, |_request_bytes, _ctx| async move {
            Err(Box::new(std::io::Error::other("test error"))
                as Box<dyn std::error::Error + Send + Sync>)
        })
        .await;
}

pub async fn register_standard_handlers<C>(endpoint: &impl RpcServiceEndpointInterface<C>)
where
    C: Send + Sync + Clone + 'static,
{
    endpoint
        .register_prebuffered(Add::METHOD_ID, |request_bytes, _ctx| async move {
            let request_params = Add::decode_request(&request_bytes)?;
            let sum = request_params.iter().sum();
            let response_bytes = Add::encode_response(sum)?;
            Ok(response_bytes)
        })
        .await
        .expect("Failed to register Add handler");
    endpoint
        .register_prebuffered(Mult::METHOD_ID, |request_bytes, _ctx| async move {
            let request_params = Mult::decode_request(&request_bytes)?;
            let product = request_params.iter().product();
            let response_bytes = Mult::encode_response(product)?;
            Ok(response_bytes)
        })
        .await
        .expect("Failed to register Mult handler");
    endpoint
        .register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
            let request_params = Echo::decode_request(&request_bytes)?;
            let response_bytes = Echo::encode_response(request_params)?;
            Ok(response_bytes)
        })
        .await
        .expect("Failed to register Echo handler");
}

/// Register a streaming handler at `STREAMING_CAPTURE_METHOD_ID` that captures
/// received events into a shared Vec for test verification.
/// Returns an `Arc<Mutex<Vec<RpcStreamEvent>>>` that the test can inspect.
pub async fn register_stream_capture_handler<C>(
    endpoint: &impl RpcServiceEndpointInterface<C>,
) -> Arc<Mutex<Vec<RpcStreamEvent>>>
where
    C: Send + Sync + Clone + 'static,
{
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let chunks: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let chunks_for_handler = chunks.clone();
    endpoint
        .register_stream_handler(STREAMING_CAPTURE_METHOD_ID, move |event, respond, _ctx| {
            captured.lock().unwrap().push(event.clone());
            match &event {
                RpcStreamEvent::PayloadChunk { bytes, .. } => {
                    chunks_for_handler.lock().unwrap().push(bytes.clone());
                }
                RpcStreamEvent::End { .. } => {
                    let all_data: Vec<u8> = chunks_for_handler
                        .lock()
                        .unwrap()
                        .drain(..)
                        .flatten()
                        .collect();
                    respond.respond(all_data, true);
                }
                _ => {}
            }
        })
        .await
        .expect("Failed to register streaming capture handler");
    events
}

pub async fn register_echo_handler<C>(endpoint: &impl RpcServiceEndpointInterface<C>)
where
    C: Send + Sync + Clone + 'static,
{
    endpoint
        .register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
            Echo::encode_response(Echo::decode_request(&request_bytes)?)
                .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
        })
        .await
        .expect("Failed to register Echo handler");
}
