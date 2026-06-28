use futures::future::join_all;
use muxio_core::{frame::FrameDecodeError, rpc::RpcDispatcher};
use muxio_rpc_service::constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::RpcServiceEndpointInterface;
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, process_single_prebuffered_request}; // Import process_single_prebuffered_request
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex;

type RpcTransportStateChangeHandler =
    Arc<Mutex<Option<Box<dyn Fn(RpcTransportState) + Send + Sync>>>>;

/// A WASM-compatible RPC client.
pub struct RpcWasmClient {
    dispatcher: Arc<tokio::sync::Mutex<RpcDispatcher<'static>>>,
    /// The endpoint for handling incoming RPC calls from the host.
    endpoint: Arc<RpcServiceEndpoint<()>>,
    emit_callback: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
    pub(crate) state_change_handler: RpcTransportStateChangeHandler,
    is_connected: Arc<AtomicBool>,
}

impl RpcWasmClient {
    pub fn new(emit_callback: impl Fn(Vec<u8>) + Send + Sync + 'static) -> RpcWasmClient {
        RpcWasmClient {
            dispatcher: Arc::new(Mutex::new(RpcDispatcher::new())),
            endpoint: Arc::new(RpcServiceEndpoint::new()),
            emit_callback: Arc::new(emit_callback),
            state_change_handler: Arc::new(Mutex::new(None)),
            is_connected: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Call this from your JavaScript glue code when the WebSocket `onopen` event fires.
    pub async fn handle_connect(&self) {
        self.is_connected.store(true, Ordering::SeqCst);
        let guard = self.state_change_handler.lock().await;
        if let Some(handler) = guard.as_ref() {
            handler(RpcTransportState::Connected);
        }
    }

    /// Call this from your JavaScript glue code when the WebSocket receives a message.
    /// This now handles both dispatcher reading and endpoint processing of incoming requests.
    pub async fn read_bytes(&self, bytes: &[u8]) {
        let dispatcher_arc = self.dispatcher.clone();
        let endpoint_arc = self.endpoint.clone();
        let emit_fn_arc = self.emit_callback.clone();

        // Stage 1: Synchronous Reading from Dispatcher (lock briefly held)
        let mut requests_to_process: Vec<(u32, muxio_core::rpc::RpcRequest)> = Vec::new();
        {
            // Acquire lock to read bytes into the dispatcher
            let mut dispatcher_guard = dispatcher_arc.lock().await;
            match dispatcher_guard.read_bytes(bytes) {
                Ok(request_ids) => {
                    for id in request_ids {
                        // Check if the request is finalized and needs processing
                        if dispatcher_guard
                            .is_rpc_request_finalized(id)
                            .unwrap_or(false)
                        {
                            // Take the request out of the dispatcher for processing
                            if let Some(req) = dispatcher_guard.delete_rpc_request(id) {
                                requests_to_process.push((id, req));
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "WASM client `read_bytes`: Dispatcher `read_bytes` error: {:?}",
                        e
                    );
                    return; // Early exit on unrecoverable read error
                }
            }
        } // IMPORTANT: `dispatcher_guard` is dropped here, releasing the lock.

        // Stage 2: Asynchronous Processing of Requests (NO dispatcher lock held)
        // This allows other tasks to potentially use the dispatcher while handlers run.
        let mut response_futures = Vec::new();
        let handlers_arc = endpoint_arc.get_prebuffered_handlers(); // Get a clone of the handlers Arc

        for (request_id, request) in requests_to_process {
            let handlers_arc_clone = handlers_arc.clone(); // Clone for each future
            let handler_context = (); // Context is () for WASM client (no per-connection state needed by handlers)

            let future = process_single_prebuffered_request(
                // This function is async and calls the user's handlers
                handlers_arc_clone,
                handler_context,
                request_id,
                request,
            );
            response_futures.push(future);
        }

        // Await all responses concurrently. This is where the bulk of the "work" happens.
        let responses_to_send = join_all(response_futures).await;

        // Stage 3: Synchronous Sending of Responses (lock briefly re-acquired)
        // Acquire lock again to write responses back to the dispatcher
        {
            let mut dispatcher_guard = dispatcher_arc.lock().await;
            for response in responses_to_send {
                let emit_fn_clone_for_respond = emit_fn_arc.clone();
                let _ = dispatcher_guard.respond(
                    response,
                    DEFAULT_SERVICE_MAX_CHUNK_SIZE, // Use the imported constant
                    move |chunk: &[u8]| {
                        // This callback is synchronous and uses the cloned emit_fn
                        emit_fn_clone_for_respond(chunk.to_vec());
                    },
                );
            }
        } // `dispatcher_guard` is dropped here.
    }

    /// Call this from your JavaScript glue code when the WebSocket's `onclose` or `onerror` event fires.
    pub async fn handle_disconnect(&self) {
        if self.is_connected.swap(false, Ordering::SeqCst) {
            let guard = self.state_change_handler.lock().await;
            if let Some(handler) = guard.as_ref() {
                handler(RpcTransportState::Disconnected);
            }
            let mut dispatcher = self.dispatcher.lock().await;
            let error = FrameDecodeError::ReadAfterCancel; // Or an appropriate disconnection error
            dispatcher.fail_all_pending_requests(error);
        }
    }

    /// A helper method to check the connection status.
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }

    pub fn get_endpoint(&self) -> Arc<RpcServiceEndpoint<()>> {
        self.endpoint.clone()
    }

    fn dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    fn emit(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        self.emit_callback.clone()
    }
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcWasmClient {
    fn get_dispatcher(&self) -> Arc<tokio::sync::Mutex<RpcDispatcher<'static>>> {
        self.dispatcher()
    }

    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        self.emit()
    }

    fn is_connected(&self) -> bool {
        self.is_connected()
    }

    async fn set_state_change_handler(
        &self,
        handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        let mut state_handler = self.state_change_handler.lock().await;
        *state_handler = Some(Box::new(handler));

        if self.is_connected()
            && let Some(h) = state_handler.as_ref()
        {
            h(RpcTransportState::Connected);
        }
    }
}
