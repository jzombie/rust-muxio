use muxio::rpc::RpcDispatcher;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use std::sync::{Arc, Mutex};

type RpcTransportStateChangeHandler = Arc<Mutex<Option<Box<dyn Fn(RpcTransportState) + Send + Sync>>>>;

/// A WASM-compatible RPC client.
pub struct RpcWasmClient {
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    emit_callback: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
    state_change_handler: RpcTransportStateChangeHandler,
}

impl RpcWasmClient {
    pub fn new(emit_callback: impl Fn(Vec<u8>) + Send + Sync + 'static) -> RpcWasmClient {
        RpcWasmClient {
            dispatcher: Arc::new(Mutex::new(RpcDispatcher::new())),
            emit_callback: Arc::new(emit_callback),
            // Initialize the handler as None
            state_change_handler: Arc::new(Mutex::new(None)),
        }
    }

    fn dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    fn emit(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        self.emit_callback.clone()
    }

    /// Provides a public accessor to the state change handler so that it can be
    /// invoked by the FFI bridge when JavaScript reports a state change.
    pub fn state_change_handler(&self) -> RpcTransportStateChangeHandler {
        self.state_change_handler.clone()
    }
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcWasmClient {
    type DispatcherLock = Mutex<RpcDispatcher<'static>>;

    fn get_dispatcher(&self) -> Arc<Self::DispatcherLock> {
        self.dispatcher()
    }

    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        self.emit()
    }

    /// Sets a callback that will be invoked with the current `RpcTransportState`
    /// whenever the underlying transport's connection status changes.
    ///
    /// Since the WASM client is not aware of the connection itself, it is the
    /// responsibility of the JavaScript host to call an FFI function (like
    /// `notify_transport_state_change`) to trigger this handler.
    fn set_state_change_handler(&self, handler: impl Fn(RpcTransportState) + Send + Sync + 'static) {
        let mut state_handler = self
            .state_change_handler
            .lock()
            .expect("Mutex should not be poisoned");
        *state_handler = Some(Box::new(handler));
    }
}
