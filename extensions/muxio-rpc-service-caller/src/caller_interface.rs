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
use std::sync::{Arc, Mutex};

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
            mpsc::Receiver<Result<Vec<u8>, Vec<u8>>>,
        ),
        io::Error,
    > {
        // The channel now sends a Result where Err contains the error payload.
        let (tx, rx) =
            mpsc::channel::<Result<Vec<u8>, Vec<u8>>>(DEFAULT_RPC_STREAM_CHANNEL_BUFFER_SIZE);
        let tx = Arc::new(Mutex::new(Some(tx)));

        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), io::Error>>();
        let ready_tx = Arc::new(Mutex::new(Some(ready_tx)));

        let send_fn: Box<dyn RpcEmit + Send + Sync> = Box::new({
            let on_emit = self.get_emit_fn();
            move |chunk: &[u8]| {
                on_emit(chunk.to_vec());
            }
        });

        // This closure is now stateful to handle error payloads correctly.
        let recv_fn: Box<dyn FnMut(RpcStreamEvent) + Send + 'static> = {
            let status = Arc::new(Mutex::new(None::<RpcResultStatus>));

            Box::new(move |evt| {
                match evt {
                    RpcStreamEvent::Header { rpc_header, .. } => {
                        let result_status = rpc_header
                            .rpc_metadata_bytes
                            .first()
                            .copied()
                            .and_then(|b| RpcResultStatus::try_from(b).ok())
                            .unwrap_or(RpcResultStatus::Success);

                        *status.lock().unwrap() = Some(result_status);

                        // Signal readiness unless there's a critical setup failure.
                        if let Some(tx) = ready_tx.lock().unwrap().take() {
                            if matches!(
                                result_status,
                                RpcResultStatus::SystemError | RpcResultStatus::MethodNotFound
                            ) {
                                let _ = tx.send(Err(io::Error::new(
                                    io::ErrorKind::Other,
                                    format!("RPC setup failed: {:?}", result_status),
                                )));
                            } else {
                                let _ = tx.send(Ok(()));
                            }
                        }
                    }
                    RpcStreamEvent::PayloadChunk { bytes, .. } => {
                        let current_status = status.lock().unwrap();
                        if let Some(sender) = tx.lock().unwrap().as_mut() {
                            // Check the status received in the header.
                            match *current_status {
                                Some(RpcResultStatus::Success) => {
                                    let _ = sender.try_send(Ok(bytes));
                                }
                                // If the status was a failure, this payload is the error message.
                                Some(RpcResultStatus::Fail) => {
                                    let _ = sender.try_send(Err(bytes));
                                }
                                // Ignore payloads for other statuses like SystemError.
                                _ => {}
                            }
                        }
                    }
                    RpcStreamEvent::End { .. } => {
                        // Close the channel by dropping the sender.
                        let _ = tx.lock().unwrap().take();
                    }
                    _ => {}
                }
            })
        };

        let encoder = self
            .get_dispatcher()
            .with_dispatcher(|d| {
                d.call(
                    RpcRequest {
                        rpc_method_id,
                        rpc_param_bytes: Some(rpc_param_bytes.to_vec()),
                        rpc_prebuffered_payload_bytes: None,
                        is_finalized,
                    },
                    DEFAULT_SERVICE_MAX_CHUNK_SIZE,
                    send_fn,
                    Some(recv_fn),
                    // Pre-buffering is now set to false to allow streaming of error payloads.
                    false,
                )
            })
            .await
            .map_err(|e: FrameEncodeError| {
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
    async fn call_rpc_buffered<T, F>(
        &self,
        method_id: u64,
        param_bytes: &[u8],
        decode: F,
        is_finalized: bool,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            Result<T, Vec<u8>>, // The error type is now Vec<u8>
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

        let mut success_buf = Vec::new();

        while let Some(result) = stream.next().await {
            match result {
                Ok(chunk) => {
                    success_buf.extend_from_slice(&chunk);
                }
                Err(error_payload) => {
                    // If we receive an error payload, the call has failed. Return it immediately.
                    return Ok((encoder, Err(error_payload)));
                }
            }
        }

        // If the stream completes without an error, decode the success buffer.
        Ok((encoder, Ok(decode(&success_buf))))
    }
}
