use super::{
    error::{HandlerPayloadError, RpcServiceEndpointError},
    with_handlers_trait::WithHandlers,
};
use futures::future::join_all;
use muxio::rpc::{RpcDispatcher, RpcResponse, rpc_internals::rpc_trait::RpcEmit};
use muxio_rpc_service::RpcResultStatus;
use muxio_rpc_service::constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE;
use std::{collections::hash_map::Entry, future::Future, marker::Send, sync::Arc};

#[async_trait::async_trait]
pub trait RpcServiceEndpointInterface<C>: Send + Sync
where
    C: Send + Sync + Clone + 'static,
{
    type HandlersLock: WithHandlers<C>;

    fn get_prebuffered_handlers(&self) -> Arc<Self::HandlersLock>;

    async fn register_prebuffered<F, Fut>(
        &self,
        method_id: u64,
        handler: F,
    ) -> Result<(), RpcServiceEndpointError>
    where
        F: Fn(C, Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>
            + Send
            + 'static,
    {
        self.get_prebuffered_handlers()
            .with_handlers(|handlers| match handlers.entry(method_id) {
                Entry::Occupied(_) => {
                    let err_msg = format!(
                        "a handler for method ID {} is already registered",
                        method_id
                    );
                    Err(RpcServiceEndpointError::Handler(err_msg.into()))
                }
                Entry::Vacant(entry) => {
                    let wrapped = move |ctx: C, bytes: Vec<u8>| {
                        Box::pin(handler(ctx, bytes))
                            as std::pin::Pin<Box<dyn Future<Output = _> + Send>>
                    };
                    entry.insert(Arc::new(wrapped));
                    Ok(())
                }
            })
            .await
    }

    /// Reads raw bytes from the transport, decodes them into RPC requests,
    /// invokes the appropriate handler, and sends back a response.
    async fn read_bytes<'a, E>(
        &self,
        dispatcher: &mut RpcDispatcher<'a>,
        context: C,
        bytes: &[u8],
        on_emit: E,
    ) -> Result<(), RpcServiceEndpointError>
    where
        E: RpcEmit + Send + Sync + Clone,
    {
        // This logic is now fully generic and reusable.
        let request_ids = dispatcher.read_bytes(bytes)?;
        let mut requests_to_process = Vec::new();
        for id in request_ids {
            if dispatcher.is_rpc_request_finalized(id).unwrap_or(false) {
                if let Some(req) = dispatcher.delete_rpc_request(id) {
                    requests_to_process.push((id, req));
                }
            }
        }

        if requests_to_process.is_empty() {
            return Ok(());
        }

        // The rest of the logic remains the same, as it was already correct.
        let handlers_arc = self.get_prebuffered_handlers();
        let mut response_futures = Vec::new();
        for (request_id, request) in requests_to_process {
            let handlers_arc_clone = handlers_arc.clone();
            let context_clone = context.clone();

            let future = async move {
                let handler = handlers_arc_clone
                    .with_handlers(|handlers| handlers.get(&request.rpc_method_id).cloned())
                    .await;
                if let Some(handler) = handler {
                    let payload = request
                        .rpc_prebuffered_payload_bytes
                        .as_deref()
                        .unwrap_or(&[]);
                    let params = request.rpc_param_bytes.as_deref().unwrap_or(&[]);
                    let args_for_handler = if !payload.is_empty() { payload } else { params };

                    match handler(context_clone, args_for_handler.to_vec()).await {
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
                    RpcResponse {
                        rpc_request_id: request_id,
                        rpc_method_id: request.rpc_method_id,
                        rpc_result_status: Some(RpcResultStatus::MethodNotFound.into()),
                        rpc_prebuffered_payload_bytes: None,
                        is_finalized: true,
                    }
                }
            };
            response_futures.push(future);
        }

        let responses = join_all(response_futures).await;
        for response in responses {
            let _ = dispatcher.respond(response, DEFAULT_SERVICE_MAX_CHUNK_SIZE, on_emit.clone());
        }

        Ok(())
    }
}
