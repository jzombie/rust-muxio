use futures::channel::oneshot;
use muxio::rpc::{
    RpcDispatcher, RpcRequest, RpcResultStatus,
    rpc_internals::{RpcStreamEncoder, RpcStreamEvent, rpc_trait::RpcEmit},
};
use muxio_rpc_service::{RpcClientInterface, constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE};
use std::io;
use std::sync::{Arc, Mutex};
use wasm_bindgen_futures::spawn_local;

pub struct RpcWasmClient {
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    emit_callback: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
}

impl RpcWasmClient {
    pub fn new(emit_callback: impl Fn(Vec<u8>) + Send + Sync + 'static) -> RpcWasmClient {
        let dispatcher = Arc::new(Mutex::new(RpcDispatcher::new()));

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
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            Result<T, io::Error>,
        ),
        io::Error,
    >
    where
        T: Send + 'static,
        F: Fn(&[u8]) -> T + Send + Sync + 'static,
    {
        let emit = self.emit_callback.clone();

        let (done_tx, done_rx) = oneshot::channel::<Result<T, io::Error>>();
        let done_tx = Arc::new(Mutex::new(Some(done_tx)));
        let done_tx_clone = done_tx.clone();

        let response_buf = Arc::new(Mutex::new(Vec::new()));
        let response_buf_clone = Arc::clone(&response_buf);

        let response_handler = Arc::new(response_handler);

        let send_fn: Box<dyn RpcEmit + Send + Sync> = Box::new(move |chunk: &[u8]| {
            emit(chunk.to_vec());
        });

        let recv_fn: Box<dyn FnMut(RpcStreamEvent) + Send + 'static> = Box::new({
            let response_handler = Arc::clone(&response_handler);
            move |evt| match evt {
                RpcStreamEvent::Header { rpc_header, .. } => {
                    let result_status = rpc_header
                        .metadata_bytes
                        .first()
                        .copied()
                        .and_then(|b| RpcResultStatus::try_from(b).ok())
                        .unwrap_or(RpcResultStatus::Success);

                    if result_status != RpcResultStatus::Success {
                        let done_tx_clone2 = done_tx_clone.clone();
                        spawn_local(async move {
                            let err = io::Error::new(
                                io::ErrorKind::Other,
                                format!("RPC call failed: {:?}", result_status),
                            );
                            if let Some(tx) = done_tx_clone2.lock().unwrap().take() {
                                let _ = tx.send(Err(err));
                            }
                        });
                    }
                }

                RpcStreamEvent::PayloadChunk { bytes, .. } => {
                    let mut buf = response_buf.lock().unwrap();
                    buf.extend_from_slice(&bytes);
                }

                RpcStreamEvent::End { .. } => {
                    let done_tx_clone2 = done_tx_clone.clone();
                    let full_response = response_buf_clone.lock().unwrap().clone();
                    let response_handler = Arc::clone(&response_handler);

                    spawn_local(async move {
                        let result = response_handler(&full_response);
                        if let Some(tx) = done_tx_clone2.lock().unwrap().take() {
                            let _ = tx.send(Ok(result));
                        }
                    });
                }

                _ => {}
            }
        });

        let rpc_stream_encoder = self
            .dispatcher
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Dispatcher lock poisoned"))?
            .call(
                RpcRequest {
                    method_id,
                    param_bytes: Some(payload.to_vec()),
                    prebuffered_payload_bytes: None,
                    is_finalized,
                },
                DEFAULT_SERVICE_MAX_CHUNK_SIZE,
                send_fn,
                Some(recv_fn),
                true,
            )
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Encode error: {e:?}")))?;

        let result = done_rx.await.map_err(|_| {
            io::Error::new(
                io::ErrorKind::Other,
                "oneshot receive failed: channel dropped",
            )
        })?;

        Ok((rpc_stream_encoder, result))
    }
}
