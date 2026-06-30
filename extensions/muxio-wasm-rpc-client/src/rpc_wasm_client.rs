use muxio_core::{frame::FrameDecodeError, rpc::RpcDispatcher};
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
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
    /// Delegates to the endpoint's `read_bytes` which handles both prebuffered and
    /// streaming handler routing.
    pub async fn read_bytes(&self, bytes: &[u8]) {
        let mut dispatcher = self.dispatcher.lock().await;
        let emit = self.emit_callback.clone();
        let _ = self
            .endpoint
            .read_bytes(&mut dispatcher, (), bytes, move |chunk: &[u8]| {
                emit(chunk.to_vec());
            })
            .await;
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
