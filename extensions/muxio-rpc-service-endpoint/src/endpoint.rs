use super::{RpcServiceEndpointInterface, error::RpcServiceEndpointError};
use muxio::frame::FrameEncodeError;
use muxio::rpc::{RpcDispatcher, RpcResponse, RpcResultStatus, rpc_internals::rpc_trait::RpcEmit};
use muxio_rpc_service::constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::marker::Send;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

pub type RpcPrebufferedHandler = Box<
    dyn Fn(
            Vec<u8>,
        ) -> Pin<
            Box<
                // TODO: Make type alias
                dyn Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

pub struct RpcServiceEndpoint {
    prebuffered_handlers: Arc<Mutex<HashMap<u64, RpcPrebufferedHandler>>>,
    rpc_dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
}

impl RpcServiceEndpoint {
    pub fn new() -> Self {
        Self {
            prebuffered_handlers: Arc::new(Mutex::new(HashMap::new())),
            rpc_dispatcher: Arc::new(Mutex::new(RpcDispatcher::new())),
        }
    }

    pub async fn read_bytes<E>(
        &self,
        bytes: &[u8],
        mut on_emit: E,
    ) -> Result<(), RpcServiceEndpointError>
    where
        E: RpcEmit + Send,
    {
        let mut rpc_dispatcher = self.rpc_dispatcher.lock().await;

        let request_ids = match rpc_dispatcher.read_bytes(&bytes) {
            Ok(ids) => ids,
            Err(e) => return Err(RpcServiceEndpointError::Decode(e)),
        };

        // Handle prebuffered requests
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

            let response = if let Some(handler) = self
                .prebuffered_handlers
                .lock()
                .await
                .get(&request.method_id)
            {
                match handler(param_bytes.clone()).await {
                    Ok(encoded) => RpcResponse {
                        request_header_id: request_id,
                        method_id: request.method_id,
                        result_status: Some(RpcResultStatus::Success.value()),
                        prebuffered_payload_bytes: Some(encoded),
                        is_finalized: true,
                    },
                    Err(e) => {
                        // TODO: Handle accordingly
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
                    result_status: Some(RpcResultStatus::SystemError.value()),
                    prebuffered_payload_bytes: None,
                    is_finalized: true,
                }
            };

            rpc_dispatcher
                .respond(
                    response,
                    DEFAULT_SERVICE_MAX_CHUNK_SIZE, // TODO: Make configurable
                    |chunk| on_emit(chunk),
                )
                .map_err(|e: FrameEncodeError| RpcServiceEndpointError::Encode(e))?;
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl RpcServiceEndpointInterface for RpcServiceEndpoint {
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
        // Acquire the lock. If it's poisoned, create a descriptive error message.
        let mut handlers = self.prebuffered_handlers.lock().await;

        // Use the entry API to atomically check and insert.
        match handlers.entry(method_id) {
            // If the key already exists, return an error.
            Entry::Occupied(_) => {
                let err_msg = format!(
                    "a handler for method ID {} is already registered",
                    method_id
                );
                Err(RpcServiceEndpointError::Handler(err_msg.into()))
            }

            // If the key doesn't exist, insert the handler and return Ok.
            Entry::Vacant(entry) => {
                let wrapped = move |bytes: Vec<u8>| {
                    Box::pin(handler(bytes)) as Pin<Box<dyn Future<Output = _> + Send>>
                };
                entry.insert(Box::new(wrapped));
                Ok(())
            }
        }
    }
}
