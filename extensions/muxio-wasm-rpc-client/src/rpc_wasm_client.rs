use futures::channel::oneshot;
use muxio::rpc::{
    RpcDispatcher, RpcRequest,
    rpc_internals::{RpcStreamEncoder, RpcStreamEvent},
};
use muxio_rpc_service::RpcClientInterface;
use std::io;
use std::sync::Arc;
use wasm_bindgen_futures::spawn_local;

pub struct RpcWasmClient {
    // TODO: Should these be kept public?
    pub dispatcher: Arc<std::sync::Mutex<RpcDispatcher<'static>>>,
    pub emit_callback: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
}

impl RpcWasmClient {
    pub fn new(emit_callback: impl Fn(Vec<u8>) + Send + Sync + 'static) -> RpcWasmClient {
        let dispatcher = Arc::new(std::sync::Mutex::new(RpcDispatcher::new()));

        RpcWasmClient {
            dispatcher,
            emit_callback: Arc::new(emit_callback),
        }
    }

    pub fn receive_inbound(&self, bytes: Vec<u8>) {
        web_sys::console::log_1(&"receive...".into());
        if let Err(e) = self.dispatcher.lock().unwrap().receive_bytes(&bytes) {
            web_sys::console::error_1(&format!("Dispatcher error: {:?}", e).into());
        }
    }
}

#[async_trait::async_trait]
impl RpcClientInterface for RpcWasmClient {
    async fn call_rpc<T, F>(
        &self,
        method_id: u64,
        payload: Vec<u8>,
        response_handler: F,
        is_finalized: bool,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn for<'a> FnMut(&'a [u8]) + Send + 'static>>,
            T,
        ),
        std::io::Error,
    >
    where
        T: Send + 'static,
        F: Fn(Vec<u8>) -> T + Send + Sync + 'static,
    {
        web_sys::console::log_1(&"Call RPC...".into());

        let emit = self.emit_callback.clone();
        let (done_tx, done_rx) = oneshot::channel::<T>();
        let done_tx = Arc::new(std::sync::Mutex::new(Some(done_tx)));
        let done_tx_clone = done_tx.clone();

        let send_fn: Box<dyn for<'a> FnMut(&'a [u8]) + Send + 'static> = Box::new(move |chunk| {
            emit(chunk.to_vec());
            web_sys::console::log_1(&"emit...".into());
        });

        let recv_fn: Box<dyn FnMut(RpcStreamEvent) + Send + 'static> = Box::new(move |evt| {
            if let RpcStreamEvent::PayloadChunk { bytes, .. } = evt {
                let result = response_handler(bytes);
                let done_tx_clone2 = done_tx_clone.clone();

                spawn_local(async move {
                    if let Some(tx) = done_tx_clone2.lock().unwrap().take() {
                        let _ = tx.send(result);
                    }
                });
            }
        });

        let rpc_stream_encoder = self
            .dispatcher
            .lock()
            .unwrap()
            .call(
                RpcRequest {
                    method_id,
                    param_bytes: Some(payload),
                    pre_buffered_payload_bytes: None,
                    is_finalized,
                },
                1024,
                send_fn,
                Some(recv_fn),
                true,
            )
            .expect("dispatcher.call failed");

        let result = done_rx.await.expect("oneshot receive failed");

        Ok((rpc_stream_encoder, result))
    }
}
