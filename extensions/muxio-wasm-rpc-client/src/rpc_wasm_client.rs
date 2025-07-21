use muxio::rpc::RpcDispatcher;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
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

    /// This method should be exposed via FFI and called by the JavaScript
    /// host whenever a binary message is received from the WebSocket.
    pub fn read_bytes(&self, bytes: &[u8]) {
        // 1. Process as a potential response to an outgoing call.
        self.dispatcher.lock().unwrap().read_bytes(bytes).ok();

        // 2. Process as a potential new incoming call from the host.
        let emit_clone = self.emit_callback.clone();
        let on_emit = move |chunk: &[u8]| {
            (emit_clone)(chunk.to_vec());
        };

        let mut local_dispatcher = RpcDispatcher::new();
        let endpoint = self.endpoint.clone();
        let bytes_owned = bytes.to_vec();

        // Use wasm_bindgen_futures to run the async block in a WASM environment.
        wasm_bindgen_futures::spawn_local(async move {
            let _ = endpoint
                .read_bytes(&mut local_dispatcher, (), &bytes_owned, on_emit)
                .await;
        });
    }

    // This is part of the struct's own implementation, not a trait.
    pub fn get_endpoint(&self) -> Arc<RpcServiceEndpoint<()>> {
        self.endpoint.clone()
    }

    fn dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    fn emit(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        self.emit_callback.clone()
    }

    /// Provides a public accessor to the state change handler.
    pub fn state_change_handler(&self) -> RpcTransportStateChangeHandler {
        self.state_change_handler.clone()
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
    fn set_state_change_handler(
        &self,
        handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        let mut state_handler = self
            .state_change_handler
            .lock()
            .expect("Mutex should not be poisoned");
        *state_handler = Some(Box::new(handler));
    }
}
