use super::{
    endpoint::RpcPrebufferedHandler, error::RpcServiceEndpointError,
    with_handlers_trait::WithHandlers,
};
use futures::future::join_all;
use muxio::rpc::{RpcDispatcher, RpcRequest, RpcResponse, rpc_internals::rpc_trait::RpcEmit};
// Corrected: RpcResultStatus comes from the service crate.
use muxio_rpc_service::RpcResultStatus;
use muxio_rpc_service::constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE;
use muxio_rpc_service_caller::WithDispatcher;
use std::{collections::hash_map::Entry, future::Future, marker::Send, sync::Arc};

#[async_trait::async_trait]
pub trait RpcServiceEndpointInterface: Send + Sync {
    type DispatcherLock: WithDispatcher;
    type HandlersLock: WithHandlers;

    fn get_dispatcher(&self) -> Arc<Self::DispatcherLock>;
    fn get_prebuffered_handlers(&self) -> Arc<Self::HandlersLock>;

    async fn register_prebuffered<F, Fut>(
        &self,
        method_id: u64,
        handler: F,
    ) -> Result<(), RpcServiceEndpointError>
    where
        F: Fn(Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>
            + Send
            + 'static,
    {
        self.get_prebuffered_handlers()
            .with_handlers(|handlers| {
                // FIX: Correct usage of the entry API.
                match handlers.entry(method_id) {
                    Entry::Occupied(_) => {
                        let err_msg = format!(
                            "a handler for method ID {} is already registered",
                            method_id
                        );
                        Err(RpcServiceEndpointError::Handler(err_msg.into()))
                    }
                    Entry::Vacant(entry) => {
                        let wrapped = move |bytes: Vec<u8>| {
                            Box::pin(handler(bytes))
                                as std::pin::Pin<Box<dyn Future<Output = _> + Send>>
                        };
                        entry.insert(Arc::new(wrapped));
                        Ok(())
                    }
                }
            })
            .await
    }

    async fn read_bytes<E>(&self, bytes: &[u8], on_emit: E) -> Result<(), RpcServiceEndpointError>
    where
        E: RpcEmit + Send + Sync + Clone,
    {
        // --- Stage 1: Decode incoming requests ---
        let (requests_to_process, handlers_arc) = {
            let dispatcher_arc = self.get_dispatcher();
            let handlers_arc = self.get_prebuffered_handlers();

            dispatcher_arc
                .with_dispatcher(move |dispatcher| {
                    let request_ids = dispatcher.read_bytes(bytes)?;
                    let mut requests_found = Vec::new();

                    for id in request_ids {
                        if dispatcher.is_rpc_request_finalized(id).unwrap_or(false) {
                            if let Some(req) = dispatcher.delete_rpc_request(id) {
                                requests_found.push((id, req));
                            }
                        }
                    }
                    // FIX: Explicitly define the Ok variant's type to resolve ambiguity.
                    Ok::<_, RpcServiceEndpointError>((requests_found, handlers_arc))
                })
                .await?
        };

        if requests_to_process.is_empty() {
            return Ok(());
        }

        // --- Stage 2: Concurrently process handlers WITHOUT holding locks ---
        let mut response_futures = Vec::new();
        for (request_header_id, request) in requests_to_process {
            let handlers_arc_clone = handlers_arc.clone();
            let future = async move {
                let handler = handlers_arc_clone
                    .with_handlers(|handlers| handlers.get(&request.method_id).cloned())
                    .await;

                if let (Some(handler), Some(params)) = (handler, &request.param_bytes) {
                    match handler(params.clone()).await {
                        Ok(encoded) => RpcResponse {
                            request_header_id,
                            method_id: request.method_id,
                            result_status: Some(RpcResultStatus::Success.into()),
                            prebuffered_payload_bytes: Some(encoded),
                            is_finalized: true,
                        },
                        Err(e) => {
                            eprintln!("Handler for method {} failed: {:?}", request.method_id, e);
                            RpcResponse {
                                request_header_id,
                                method_id: request.method_id,
                                result_status: Some(RpcResultStatus::SystemError.into()),
                                prebuffered_payload_bytes: None,
                                is_finalized: true,
                            }
                        }
                    }
                } else {
                    RpcResponse {
                        request_header_id,
                        method_id: request.method_id,
                        result_status: Some(RpcResultStatus::MethodNotFound.into()),
                        prebuffered_payload_bytes: None,
                        is_finalized: true,
                    }
                }
            };
            response_futures.push(future);
        }

        let responses = join_all(response_futures).await;

        // --- Stage 3: Lock the dispatcher again to send all responses ---
        self.get_dispatcher()
            .with_dispatcher(|dispatcher| {
                for response in responses {
                    dispatcher.respond(
                        response,
                        DEFAULT_SERVICE_MAX_CHUNK_SIZE,
                        on_emit.clone(),
                    )?;
                }
                Ok(())
            })
            .await
    }
}
