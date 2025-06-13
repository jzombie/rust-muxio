use super::with_dispatcher_trait::WithDispatcher;
use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use muxio::frame::FrameEncodeError;
use muxio::rpc::{
    RpcRequest,
    rpc_internals::{RpcStreamEncoder, RpcStreamEvent, rpc_trait::RpcEmit},
};
use muxio_rpc_service::RpcResultStatus;
use muxio_rpc_service::constants::{
    DEFAULT_RPC_STREAM_CHANNEL_BUFFER_SIZE, DEFAULT_SERVICE_MAX_CHUNK_SIZE,
};
use std::io;
use std::sync::Arc;

/// Defines a generic capability for making RPC calls.
///
/// Any struct that can provide an `RpcDispatcher` and an `on_emit` function
/// (e.g., a client or a server acting as a client) can implement this trait
/// to gain the ability to make outbound streaming and buffered RPC calls.
#[async_trait::async_trait]
pub trait RpcServiceCallerInterface: Send + Sync {
    /// The specific Mutex type used to protect the dispatcher.
    type DispatcherLock: WithDispatcher;

    // --- METHODS TO BE IMPLEMENTED BY THE STRUCT (e.g., RpcClient) ---

    /// A required method that provides access to the shared dispatcher.
    fn get_dispatcher(&self) -> Arc<Self::DispatcherLock>;

    /// A required method that provides the function for emitting raw bytes
    /// over the underlying transport.
    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync>;

    // --- METHODS PROVIDED AUTOMATICALLY BY THE TRAIT ---

    /// Performs a streaming RPC call.
    /// This default method uses the required getters to orchestrate the call.
    async fn call_rpc_streaming(
        &self,
        rpc_method_id: u64,
        rpc_param_bytes: &[u8], // TODO: Make this `Option` type
        // TODO: Add `prebuffered_payload_bytes` (match `RpcDispatcher` in design)
        is_finalized: bool,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            mpsc::Receiver<Vec<u8>>,
        ),
        io::Error,
    > {
        let (tx, rx) = mpsc::channel::<Vec<u8>>(DEFAULT_RPC_STREAM_CHANNEL_BUFFER_SIZE);
        let tx = Arc::new(std::sync::Mutex::new(Some(tx)));

        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), io::Error>>();
        let ready_tx = Arc::new(std::sync::Mutex::new(Some(ready_tx)));

        // TODO: Does this really have to wrap here?
        let send_fn: Box<dyn RpcEmit + Send + Sync> = Box::new({
            let on_emit = self.get_emit_fn(); // Get emit fn from the implementor
            move |chunk: &[u8]| {
                on_emit(chunk.to_vec());
            }
        });

        let recv_fn: Box<dyn FnMut(RpcStreamEvent) + Send + 'static> = Box::new(move |evt| {
            match evt {
                RpcStreamEvent::Header { rpc_header, .. } => {
                    let result_status = rpc_header
                        .rpc_metadata_bytes
                        .first()
                        .copied()
                        .and_then(|b| RpcResultStatus::try_from(b).ok())
                        .unwrap_or(RpcResultStatus::Success);

                    if result_status != RpcResultStatus::Success {
                        // TODO: Don't use `unwrap`
                        if let Some(tx) = ready_tx.lock().unwrap().take() {
                            let _ = tx.send(Err(io::Error::new(
                                io::ErrorKind::Other,
                                format!("RPC failed: {:?}", result_status),
                            )));
                        }
                        // TODO: Don't use `unwrap`
                        let _ = tx.lock().unwrap().take();
                    } else {
                        // TODO: Don't use `unwrap`
                        let _ = ready_tx.lock().unwrap().take().map(|t| t.send(Ok(())));
                    }
                }
                RpcStreamEvent::PayloadChunk { bytes, .. } => {
                    // TODO: Don't use `unwrap`
                    if let Some(sender) = tx.lock().unwrap().as_mut() {
                        let _ = sender.try_send(bytes);
                    }
                }
                RpcStreamEvent::End { .. } => {
                    // TODO: Don't use `unwrap`
                    let _ = tx.lock().unwrap().take();
                }
                _ => {
                    // TODO: Handle unmatched condition?
                }
            }
        });

        let rpc_call_result = self
            .get_dispatcher() // Get dispatcher from the implementor
            .with_dispatcher(|d| {
                d.call(
                    RpcRequest {
                        rpc_method_id,
                        rpc_param_bytes: Some(rpc_param_bytes.to_vec()),
                        rpc_prebuffered_payload_bytes: None, // TODO: Send, if attached
                        is_finalized,
                    },
                    DEFAULT_SERVICE_MAX_CHUNK_SIZE,
                    send_fn,
                    Some(recv_fn),
                    true,
                )
            })
            .await;

        let encoder = rpc_call_result.map_err(|e: FrameEncodeError| {
            io::Error::new(io::ErrorKind::Other, format!("Encode error: {e:?}"))
        })?;

        match ready_rx.await {
            Ok(Ok(())) => Ok((encoder, rx)),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(io::Error::new(
                io::ErrorKind::Other,
                "RPC response channel closed prematurely",
            )),
        }
    }

    /// Performs a buffered RPC call.
    /// This default method simply builds on top of `call_rpc_streaming`.
    async fn call_rpc_buffered<T, F>(
        &self,
        method_id: u64,
        param_bytes: &[u8],
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
            .call_rpc_streaming(method_id, param_bytes, is_finalized)
            .await?;

        let mut buf = Vec::new();
        while let Some(chunk) = stream.next().await {
            buf.extend_from_slice(&chunk);
        }

        Ok((encoder, Ok(decode(&buf))))
    }
}
