use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use muxio::rpc::{
    RpcDispatcher, RpcRequest, RpcResultStatus,
    rpc_internals::{RpcStreamEncoder, RpcStreamEvent, rpc_trait::RpcEmit},
};
use muxio_rpc_service::constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;

pub async fn call_rpc_streaming_generic(
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
            DEFAULT_SERVICE_MAX_CHUNK_SIZE, // TODO: Make configurable
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
