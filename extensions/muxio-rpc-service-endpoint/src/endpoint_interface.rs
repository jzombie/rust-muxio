use super::{endpoint::RpcPrebufferedHandler, error::RpcServiceEndpointError};
use muxio::rpc::{RpcDispatcher, RpcResponse, RpcResultStatus, rpc_internals::rpc_trait::RpcEmit};
use muxio_rpc_service::constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE;
use std::{
    collections::{HashMap, hash_map::Entry},
    future::Future,
    marker::Send,
    sync::Arc,
};
use tokio::sync::Mutex;

#[async_trait::async_trait]
pub trait RpcServiceEndpointInterface: Send + Sync {
    fn get_dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>>;
    // The HashMap now correctly contains `Arc`-wrapped handlers.
    fn get_prebuffered_handlers(&self) -> Arc<Mutex<HashMap<u64, RpcPrebufferedHandler>>>;

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
        let handlers_arc = self.get_prebuffered_handlers();
        let mut handlers = handlers_arc.lock().await;

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
                    Box::pin(handler(bytes)) as std::pin::Pin<Box<dyn Future<Output = _> + Send>>
                };
                // FIX: This now compiles because the HashMap's value type is Arc<...>.
                entry.insert(Arc::new(wrapped));
                Ok(())
            }
        }
    }

    async fn read_bytes<E>(&self, bytes: &[u8], on_emit: E) -> Result<(), RpcServiceEndpointError>
    where
        E: RpcEmit + Send + Clone,
    {
        let dispatcher_arc = self.get_dispatcher();
        let mut rpc_dispatcher = dispatcher_arc.lock().await;

        let request_ids = rpc_dispatcher
            .read_bytes(bytes)
            .map_err(RpcServiceEndpointError::Decode)?;

        let handlers_arc = self.get_prebuffered_handlers();

        for request_id in request_ids {
            if !rpc_dispatcher
                .is_rpc_request_finalized(request_id)
                .unwrap_or(false)
            {
                continue;
            }

            let Some(request) = rpc_dispatcher.delete_rpc_request(request_id) else {
                continue;
            };
            let Some(param_bytes) = &request.param_bytes else {
                continue;
            };

            let handler_to_call = {
                let handlers = handlers_arc.lock().await;
                // FIX: This now compiles because the HashMap's value type is Arc, which is Clone.
                handlers.get(&request.method_id).cloned()
            };

            let response = if let Some(handler) = handler_to_call {
                match handler(param_bytes.clone()).await {
                    Ok(encoded) => RpcResponse {
                        request_header_id: request_id,
                        method_id: request.method_id,
                        result_status: Some(RpcResultStatus::Success.value()),
                        prebuffered_payload_bytes: Some(encoded),
                        is_finalized: true,
                    },
                    Err(e) => {
                        eprintln!("Handler error: {:?}", e);
                        RpcResponse {
                            request_header_id: request_id,
                            method_id: request.method_id,
                            result_status: Some(RpcResultStatus::SystemError.value()),
                            prebuffered_payload_bytes: None,
                            is_finalized: true,
                        }
                    }
                }
            } else {
                RpcResponse {
                    request_header_id: request_id,
                    method_id: request.method_id,
                    result_status: Some(RpcResultStatus::MethodNotFound.value()),
                    prebuffered_payload_bytes: None,
                    is_finalized: true,
                }
            };

            rpc_dispatcher
                .respond(response, DEFAULT_SERVICE_MAX_CHUNK_SIZE, on_emit.clone())
                .map_err(|e| RpcServiceEndpointError::Encode(e))?;
        }
        Ok(())
    }
}
