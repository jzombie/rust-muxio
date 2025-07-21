use super::{error::HandlerPayloadError, with_handlers_trait::WithHandlers};
use muxio::rpc::{RpcRequest, RpcResponse};
use muxio_rpc_service::RpcResultStatus;
use std::sync::Arc;

/// Processes a single finalized RPC request, executes its handler, and returns the response.
///
/// This function encapsulates the logic for handler lookup, execution, and error mapping
/// for pre-buffered RPC calls, making it reusable across different endpoint implementations.
/// It assumes the `RpcRequest` has already been fully received and extracted from the dispatcher.
pub async fn process_single_prebuffered_request<C, H>(
    handlers_lock: Arc<H>, // Accepts the generic handlers lock
    context: C,
    request_id: u32, // Request ID (u32, consistent with muxio::rpc)
    request: RpcRequest,
) -> RpcResponse
where
    C: Send + Sync + Clone + 'static,
    H: WithHandlers<C> + Send + Sync + 'static,
{
    // Acquire handler map lock briefly using with_handlers
    let handler = handlers_lock
        .with_handlers(|handlers| handlers.get(&request.rpc_method_id).cloned())
        .await;

    if let Some(handler) = handler {
        let payload = request
            .rpc_prebuffered_payload_bytes
            .as_deref()
            .unwrap_or(&[]);
        let params = request.rpc_param_bytes.as_deref().unwrap_or(&[]);
        let args_for_handler = if !payload.is_empty() { payload } else { params };

        // Call the actual user-defined async handler. This might also `await` internally.
        match handler(context, args_for_handler.to_vec()).await {
            Ok(encoded) => RpcResponse {
                rpc_request_id: request_id,
                rpc_method_id: request.rpc_method_id,
                rpc_result_status: Some(RpcResultStatus::Success.into()),
                rpc_prebuffered_payload_bytes: Some(encoded),
                is_finalized: true,
            },
            Err(e) => {
                if let Some(payload_error) = e.downcast_ref::<HandlerPayloadError>() {
                    RpcResponse {
                        rpc_request_id: request_id,
                        rpc_method_id: request.rpc_method_id,
                        rpc_result_status: Some(RpcResultStatus::Fail.into()),
                        rpc_prebuffered_payload_bytes: Some(payload_error.0.clone()),
                        is_finalized: true,
                    }
                } else {
                    RpcResponse {
                        rpc_request_id: request_id,
                        rpc_method_id: request.rpc_method_id,
                        rpc_result_status: Some(RpcResultStatus::SystemError.into()),
                        rpc_prebuffered_payload_bytes: Some(e.to_string().into_bytes()),
                        is_finalized: true,
                    }
                }
            }
        }
    } else {
        // Method not found on the client's endpoint
        RpcResponse {
            rpc_request_id: request_id,
            rpc_method_id: request.rpc_method_id,
            rpc_result_status: Some(RpcResultStatus::MethodNotFound.into()),
            rpc_prebuffered_payload_bytes: None,
            is_finalized: true,
        }
    }
}
