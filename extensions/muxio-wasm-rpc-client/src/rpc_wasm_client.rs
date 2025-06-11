use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use muxio::rpc::{
    RpcDispatcher, RpcRequest, RpcResultStatus,
    rpc_internals::{RpcStreamEncoder, RpcStreamEvent, rpc_trait::RpcEmit},
};
use muxio_rpc_service::{RpcClientInterface, constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE};
use std::io;
use std::sync::{Arc, Mutex};

pub struct RpcWasmClient {
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    emit_callback: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
}

impl RpcWasmClient {
    pub fn new(emit_callback: impl Fn(Vec<u8>) + Send + Sync + 'static) -> RpcWasmClient {
        RpcWasmClient {
            dispatcher: Arc::new(Mutex::new(RpcDispatcher::new())),
            emit_callback: Arc::new(emit_callback),
        }
    }

    fn dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    fn emit(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        self.emit_callback.clone()
    }
}

#[async_trait::async_trait]
impl RpcClientInterface for RpcWasmClient {
    type DispatcherMutex<T> = Mutex<RpcDispatcher<'static>>;

    fn get_dispatcher(&self) -> Arc<Self::DispatcherMutex<RpcDispatcher<'static>>> {
        self.dispatcher()
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
        call_rpc_streaming_generic(
            self.dispatcher(),
            self.emit(),
            method_id,
            payload,
            is_finalized,
        )
        .await
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
            buf.extend_from_slice(&chunk);
        }

        Ok((encoder, Ok(decode(&buf))))
    }
}

// === Generic internals ===

async fn call_rpc_streaming_generic(
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    emit: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
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
    let (tx, rx) = mpsc::channel::<Vec<u8>>(8);
    let tx = Arc::new(Mutex::new(Some(tx)));

    let (ready_tx, ready_rx) = oneshot::channel::<Result<(), io::Error>>();
    let ready_tx = Arc::new(Mutex::new(Some(ready_tx)));

    let send_fn: Box<dyn RpcEmit + Send + Sync> = Box::new({
        let emit = emit.clone();
        move |chunk: &[u8]| {
            emit(chunk.to_vec());
        }
    });

    let recv_fn: Box<dyn FnMut(RpcStreamEvent) + Send + 'static> = Box::new({
        let tx = Arc::clone(&tx);
        let ready_tx = Arc::clone(&ready_tx);

        move |evt| match evt {
            RpcStreamEvent::Header { rpc_header, .. } => {
                let result_status = rpc_header
                    .metadata_bytes
                    .first()
                    .copied()
                    .and_then(|b| RpcResultStatus::try_from(b).ok())
                    .unwrap_or(RpcResultStatus::Success);

                if result_status != RpcResultStatus::Success {
                    if let Some(tx) = ready_tx.lock().unwrap().take() {
                        let _ = tx.send(Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("RPC failed: {:?}", result_status),
                        )));
                    }
                    let _ = tx.lock().unwrap().take();
                } else {
                    let _ = ready_tx.lock().unwrap().take().map(|t| t.send(Ok(())));
                }
            }

            RpcStreamEvent::PayloadChunk { bytes, .. } => {
                if let Some(sender) = tx.lock().unwrap().as_mut() {
                    let _ = sender.try_send(bytes);
                }
            }

            RpcStreamEvent::End { .. } => {
                let _ = tx.lock().unwrap().take();
            }

            _ => {}
        }
    });

    let encoder = dispatcher
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

    match ready_rx.await {
        Ok(Ok(())) => Ok((encoder, rx)),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::Other,
            "RPC response channel closed prematurely",
        )),
    }
}
