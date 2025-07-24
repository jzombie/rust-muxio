use muxio::{frame::FrameDecodeError, rpc::RpcDispatcher};
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::RpcServiceEndpoint;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

type RpcTransportStateChangeHandler =
    Arc<Mutex<Option<Box<dyn Fn(RpcTransportState) + Send + Sync>>>>;

/// A WASM-compatible RPC client.
pub struct RpcWasmClient {
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    /// The endpoint for handling incoming RPC calls from the host.
    endpoint: Arc<RpcServiceEndpoint<()>>,
    emit_callback: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
    // MODIFIED: Made `pub(crate)` for direct access within the crate.
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
    pub fn handle_connect(&self) {
        self.is_connected.store(true, Ordering::SeqCst);
        if let Ok(guard) = self.state_change_handler.lock() {
            if let Some(handler) = guard.as_ref() {
                handler(RpcTransportState::Connected);
            }
        }
    }

    /// Call this from your JavaScript glue code when the WebSocket receives a message.
    pub fn process_incoming_bytes(&self, bytes: &[u8]) {
        if let Ok(mut dispatcher) = self.dispatcher.lock() {
            let _ = dispatcher.read_bytes(bytes);
        }
    }

    /// Call this from your JavaScript glue code when the WebSocket's `onclose` or `onerror` event fires.
    pub fn handle_disconnect(&self) {
        if self.is_connected.swap(false, Ordering::SeqCst) {
            if let Ok(guard) = self.state_change_handler.lock() {
                if let Some(handler) = guard.as_ref() {
                    handler(RpcTransportState::Disconnected);
                }
            }
            if let Ok(mut dispatcher) = self.dispatcher.lock() {
                let error = FrameDecodeError::ReadAfterCancel;
                dispatcher.fail_all_pending_requests(error);
            }
        }
    }

    /// A helper method to check the connection status.
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }

    // This is part of the struct's own implementation, not a trait.
    pub fn get_endpoint(&self) -> Arc<RpcServiceEndpoint<()>> {
        self.endpoint.clone()
    }

    // Private accessors for the trait impl
    fn dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    fn emit(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        self.emit_callback.clone()
    }
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcWasmClient {
    // type DispatcherLock = Mutex<RpcDispatcher<'static>>;

    fn get_dispatcher(&self) -> Arc<Self::DispatcherLock> {
        self.dispatcher()
    }

    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        self.emit()
    }

    /// Sets a callback that will be invoked with the current `RpcTransportState`.
    async fn set_state_change_handler(
        &self,
        handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        let mut state_handler = self
            .state_change_handler
            .lock()
            .expect("Mutex should not be poisoned");

        *state_handler = Some(Box::new(handler));

        // Immediately call the handler with the current state.
        if self.is_connected() {
            if let Some(h) = state_handler.as_ref() {
                h(RpcTransportState::Connected);
            }
        }
    }
}
