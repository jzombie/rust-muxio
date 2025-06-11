use futures::channel::oneshot;
use muxio::rpc::{
    RpcDispatcher, RpcRequest,
    rpc_internals::{RpcStreamEncoder, RpcStreamEvent, rpc_trait::RpcEmit},
};
use muxio_rpc_service::{RpcClientInterface, constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE};
use std::sync::{Arc, Mutex};
use wasm_bindgen_futures::spawn_local;

pub struct RpcWasmClient {
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    emit_callback: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
}

impl RpcWasmClient {
    pub fn new(emit_callback: impl Fn(Vec<u8>) + Send + Sync + 'static) -> RpcWasmClient {
        let dispatcher = Arc::new(std::sync::Mutex::new(RpcDispatcher::new()));

        RpcWasmClient {
            dispatcher,
            emit_callback: Arc::new(emit_callback),
        }
    }
}

#[async_trait::async_trait]
impl RpcClientInterface for RpcWasmClient {
    type DispatcherMutex<T> = Mutex<RpcDispatcher<'static>>;

    fn get_dispatcher(&self) -> Arc<Self::DispatcherMutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    async fn call_rpc<T, F>(
        &self,
        method_id: u64,
        payload: &[u8],
        response_handler: F,
        is_finalized: bool,
    ) -> Result<(RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>, T), std::io::Error>
    where
        T: Send + 'static,
        F: Fn(&[u8]) -> T + Send + Sync + 'static,
    {
        let emit = self.emit_callback.clone();
        let (done_tx, done_rx) = oneshot::channel::<T>();
        let done_tx = Arc::new(std::sync::Mutex::new(Some(done_tx)));
        let done_tx_clone = done_tx.clone();

        let send_fn: Box<dyn RpcEmit + Send + Sync> = Box::new(move |chunk: &[u8]| {
            emit(chunk.to_vec());
        });

        let recv_fn: Box<dyn FnMut(RpcStreamEvent) + Send + 'static> = Box::new(move |evt| {
            if let RpcStreamEvent::PayloadChunk { bytes, .. } = evt {
                let result = response_handler(&bytes);
                let done_tx_clone2 = done_tx_clone.clone();

                spawn_local(async move {
                    // TODO: Don't use unwrap
                    if let Some(tx) = done_tx_clone2.lock().unwrap().take() {
                        let _ = tx.send(result);
                    }
                });
            }
        });

        let rpc_stream_encoder = self
            .dispatcher
            .lock()
            // TODO: Don't use unwrap
            .unwrap()
            .call(
                RpcRequest {
                    method_id,
                    param_bytes: Some(payload.to_vec()),
                    prebuffered_payload_bytes: None,
                    is_finalized,
                },
                DEFAULT_SERVICE_MAX_CHUNK_SIZE, // TODO: Make configurable
                send_fn,
                Some(recv_fn),
                true,
            )
            .expect("dispatcher.call failed");

        let result = done_rx.await.expect("oneshot receive failed");

        Ok((rpc_stream_encoder, result))
    }
}
