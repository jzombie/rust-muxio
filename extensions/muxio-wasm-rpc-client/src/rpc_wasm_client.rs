use muxio::{frame::FrameDecodeError, rpc::RpcDispatcher};
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::RpcServiceEndpoint;
use std::sync::{Arc, Mutex};

type RpcTransportStateChangeHandler =
    Arc<Mutex<Option<Box<dyn Fn(RpcTransportState) + Send + Sync>>>>;

/// A WASM-compatible RPC client.
pub struct RpcWasmClient {
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    /// The endpoint for handling incoming RPC calls from the host.
    endpoint: Arc<RpcServiceEndpoint<()>>,
    emit_callback: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
    state_change_handler: RpcTransportStateChangeHandler,
}

impl RpcWasmClient {
    pub fn new(emit_callback: impl Fn(Vec<u8>) + Send + Sync + 'static) -> RpcWasmClient {
        RpcWasmClient {
            dispatcher: Arc::new(Mutex::new(RpcDispatcher::new())),
            endpoint: Arc::new(RpcServiceEndpoint::new()),
            emit_callback: Arc::new(emit_callback),
            state_change_handler: Arc::new(Mutex::new(None)),
        }
    }

    /// **NEW**: Call this from your JavaScript glue code when the WebSocket's `onclose` or `onerror` event fires.
    pub fn handle_disconnect(&self) {
        // 1. Notify listeners that the transport state has changed.
        if let Ok(guard) = self.state_change_handler.lock() {
            if let Some(handler) = guard.as_ref() {
                handler(RpcTransportState::Disconnected);
            }
        }

        // 2. Fail all pending requests to prevent hangs.
        if let Ok(mut dispatcher) = self.dispatcher.lock() {
            let error = FrameDecodeError::ReadAfterCancel; // Represents a closed connection
            dispatcher.fail_all_pending_requests(error);
        }
    }

    // This is part of the struct's own implementation, not a trait.
    pub fn get_endpoint(&self) -> Arc<RpcServiceEndpoint<()>> {
        self.endpoint.clone()
    }

    /// **NEW**: Provides public access to the state change handler `Arc<Mutex<...>>`.
    pub fn state_change_handler(&self) -> RpcTransportStateChangeHandler {
        self.state_change_handler.clone()
    }

    // Private accessors for the trait impl
    fn dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    fn emit(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        self.emit_callback.clone()
    }
}

#[async_trait::async_trait(?Send)]
impl RpcServiceCallerInterface for RpcWasmClient {
    type DispatcherLock = Mutex<RpcDispatcher<'static>>;

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
        if let Ok(mut state_handler) = self.state_change_handler.lock() {
            *state_handler = Some(Box::new(handler));
        }
    }
}
