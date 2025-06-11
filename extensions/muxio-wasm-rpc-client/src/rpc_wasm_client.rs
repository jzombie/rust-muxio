use futures::StreamExt;
use futures::channel::mpsc;
use muxio::rpc::{
    RpcDispatcher, RpcRequest, RpcResultStatus,
    rpc_internals::{RpcStreamEncoder, RpcStreamEvent, rpc_trait::RpcEmit},
};
use muxio_rpc_service::{RpcClientInterface, constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE};
use std::io;
use std::sync::{Arc, Mutex};

const ERROR_MARKER_PREFIX: &[u8] = b"__MUXIO_ERR__";

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

    async fn call_rpc_streaming(
        &self,
        method_id: u64,
        payload: &[u8],
        is_finalized: bool,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            mpsc::Receiver<Vec<u8>>,
        ),
        io::Error,
    > {
        let emit = self.emit_callback.clone();
        let (tx, rx) = mpsc::channel::<Vec<u8>>(8);
        let tx = Arc::new(Mutex::new(Some(tx)));

        let send_fn: Box<dyn RpcEmit + Send + Sync> = Box::new(move |chunk: &[u8]| {
            emit(chunk.to_vec());
        });

        let recv_fn: Box<dyn FnMut(RpcStreamEvent) + Send + 'static> = Box::new({
            let tx = Arc::clone(&tx);
            move |evt| match evt {
                RpcStreamEvent::Header { rpc_header, .. } => {
                    let result_status = rpc_header
                        .metadata_bytes
                        .first()
                        .copied()
                        .and_then(|b| RpcResultStatus::try_from(b).ok())
                        .unwrap_or(RpcResultStatus::Success);

                    if result_status != RpcResultStatus::Success {
                        let _ = tx.lock().unwrap().take().map(|mut t| {
                            let mut marker = Vec::from(ERROR_MARKER_PREFIX);
                            marker.push(result_status as u8);
                            let _ = t.try_send(marker);
                        });
                    }
                }

                RpcStreamEvent::PayloadChunk { bytes, .. } => {
                    let _ = tx.lock().unwrap().as_mut().map(|t| {
                        let _ = t.try_send(bytes);
                    });
                }

                RpcStreamEvent::End { .. } => {
                    let _ = tx.lock().unwrap().take();
                }

                _ => {}
            }
        });

        let encoder = self
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

        Ok((encoder, rx))
    }

    async fn call_rpc_buffered<T, F>(
        &self,
        method_id: u64,
        payload: &[u8],
        decode: F,
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
        let (encoder, mut stream) = self
            .call_rpc_streaming(method_id, payload, is_finalized)
            .await?;

        let mut buf = Vec::new();
        while let Some(chunk) = stream.next().await {
            if chunk.starts_with(ERROR_MARKER_PREFIX) {
                let status_byte = chunk[ERROR_MARKER_PREFIX.len()];
                let err = io::Error::new(
                    io::ErrorKind::Other,
                    format!("RPC error status: {:?}", status_byte),
                );
                return Ok((encoder, Err(err)));
            }

            buf.extend_from_slice(&chunk);
        }

        Ok((encoder, Ok(decode(&buf))))
    }
}
