use crate::{
    RpcTransportState,
    dynamic_channel::{DynamicChannelType, DynamicReceiver, DynamicSender},
};
use futures::{StreamExt, channel::mpsc, channel::oneshot};
use muxio_core::rpc::{
    RpcDispatcher, RpcRequest,
    rpc_internals::{
        RpcStreamEncoder, RpcStreamEvent,
        rpc_trait::{RpcEmit, RpcResponseHandler},
    },
};
use muxio_rpc_service::{
    RpcResultStatus,
    constants::{DEFAULT_RPC_STREAM_CHANNEL_BUFFER_SIZE, DEFAULT_SERVICE_MAX_CHUNK_SIZE},
    error::{RpcServiceError, RpcServiceErrorCode, RpcServiceErrorPayload},
};
use std::{
    io, mem,
    sync::{Arc, Mutex as StdMutex},
};
use tokio::sync::Mutex as TokioMutex;
use tracing::{self, instrument};

#[async_trait::async_trait]
pub trait RpcServiceCallerInterface: Send + Sync {
    // This uses TokioMutex, which is fine for async methods using .lock().await
    fn get_dispatcher(&self) -> Arc<TokioMutex<RpcDispatcher<'static>>>;
    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync>;
    fn is_connected(&self) -> bool;

    #[instrument(skip(self, request))]
    async fn call_rpc_streaming(
        &self,
        request: RpcRequest,
        dynamic_channel_type: DynamicChannelType,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            DynamicReceiver,
        ),
        RpcServiceError,
    > {
        if !self.is_connected() {
            tracing::debug!(
                "Client is disconnected. Rejecting call immediately for method ID: {}.",
                request.rpc_method_id
            );
            return Err(RpcServiceError::Transport(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "RPC call attempted on a disconnected client.",
            )));
        }

        tracing::debug!("Starting for method ID: {}", request.rpc_method_id);
        let (tx, rx) = match dynamic_channel_type {
            DynamicChannelType::Unbounded => {
                let (sender, receiver) = mpsc::unbounded();
                tracing::debug!("Created Unbounded channel.");
                (
                    DynamicSender::Unbounded(sender),
                    DynamicReceiver::Unbounded(receiver),
                )
            }
            DynamicChannelType::Bounded => {
                let (sender, receiver) = mpsc::channel(DEFAULT_RPC_STREAM_CHANNEL_BUFFER_SIZE);
                tracing::debug!("Created Bounded channel.");
                (
                    DynamicSender::Bounded(sender),
                    DynamicReceiver::Bounded(receiver),
                )
            }
        };

        // These variables will be captured by recv_fn, so they need to use StdMutex
        // instead of TokioMutex, for synchronous locking.
        let tx_arc = Arc::new(StdMutex::new(Some(tx))); // <--- USE StdMutex HERE
        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), io::Error>>();
        let ready_tx_arc = Arc::new(StdMutex::new(Some(ready_tx))); // <--- USE StdMutex HERE
        tracing::debug!("Oneshot channel for readiness created.");

        let send_fn: Box<dyn RpcEmit + Send + Sync> = Box::new({
            tracing::trace!("`send_fn` invoked");

            let on_emit = self.get_emit_fn();
            move |chunk: &[u8]| {
                on_emit(chunk.to_vec());
            }
        });

        let recv_fn: Box<dyn RpcResponseHandler + Send + 'static> = {
            tracing::trace!("`recv_fn` invoked");

            // These internal mutexes also need to be StdMutex
            let status = Arc::new(StdMutex::new(None::<RpcResultStatus>)); // <--- USE StdMutex HERE
            let error_buffer = Arc::new(StdMutex::new(Vec::new())); // <--- USE StdMutex HERE
            let method_id = request.rpc_method_id;

            let tx_clone_for_recv_fn = tx_arc.clone();
            let ready_tx_clone_for_recv_fn = ready_tx_arc.clone();

            Box::new(move |evt| {
                // This closure is SYNCHRONOUS
                tracing::trace!(
                    "[recv_fn for method: {}] Received event: {:?}",
                    method_id,
                    evt
                );

                // Acquire std::sync::Mutexes using .lock().unwrap()
                // This will block the thread, but won't panic in WASM.
                let mut tx_lock_guard = tx_clone_for_recv_fn.lock().unwrap(); // <--- USE .lock().unwrap()
                let mut status_lock_guard = status.lock().unwrap(); // <--- USE .lock().unwrap()
                let mut ready_tx_lock_guard = ready_tx_clone_for_recv_fn.lock().unwrap(); // <--- USE .lock().unwrap()
                let mut error_buffer_lock_guard = error_buffer.lock().unwrap(); // <--- USE .lock().unwrap()

                // --- Existing recv_fn logic goes here, operating on the guards ---
                match evt {
                    RpcStreamEvent::Header { rpc_header, .. } => {
                        let result_status = rpc_header
                            .rpc_metadata_bytes
                            .first()
                            .copied()
                            .and_then(|b| RpcResultStatus::try_from(b).ok())
                            .unwrap_or(RpcResultStatus::Success);
                        *status_lock_guard = Some(result_status);
                        let mut temp_ready_tx_option = mem::take(&mut *ready_tx_lock_guard);
                        if let Some(tx_sender) = temp_ready_tx_option.take() {
                            let _ = tx_sender.send(Ok(()));
                            tracing::trace!(
                                "[recv_fn for method: {}] Sent readiness signal.",
                                method_id
                            );
                        }
                    }
                    RpcStreamEvent::PayloadChunk { bytes, .. } => {
                        let bytes_len = bytes.len();
                        let current_status_option = mem::take(&mut *status_lock_guard);
                        match current_status_option.as_ref() {
                            Some(RpcResultStatus::Success) => {
                                let mut temp_tx_option = mem::take(&mut *tx_lock_guard);
                                if let Some(sender) = temp_tx_option.as_mut() {
                                    sender.send_and_ignore(Ok(bytes));
                                    tracing::trace!(
                                        "[recv_fn for method: {}] Sent payload chunk ({} bytes) to DynamicSender.",
                                        method_id,
                                        bytes_len
                                    );
                                }
                                *tx_lock_guard = temp_tx_option;
                            }
                            Some(_) => {
                                error_buffer_lock_guard.extend(bytes);
                                tracing::trace!(
                                    "[recv_fn for method: {}] Buffered error payload chunk ({} bytes).",
                                    method_id,
                                    bytes_len
                                );
                            }
                            None => {
                                tracing::trace!(
                                    "[recv_fn for method: {}] Received payload before status. Buffering.",
                                    method_id
                                );
                                error_buffer_lock_guard.extend(bytes);
                                tracing::trace!(
                                    "[recv_fn for method {}] Buffered payload chunk ({} bytes) before status.",
                                    method_id,
                                    bytes_len
                                );
                            }
                        }
                        *status_lock_guard = current_status_option;
                    }
                    RpcStreamEvent::End { .. } => {
                        tracing::trace!("[recv_fn for method: {}] Received End event.", method_id);
                        let final_status = mem::take(&mut *status_lock_guard);

                        // FIXME: This replacement is indeed okay?
                        // let payload = std::mem::replace(&mut *error_buffer_lock_guard, Vec::new());
                        let payload = std::mem::take(&mut *error_buffer_lock_guard);

                        let mut temp_tx_option = mem::take(&mut *tx_lock_guard);
                        if let Some(mut sender) = temp_tx_option.take() {
                            match final_status {
                                Some(RpcResultStatus::MethodNotFound) => {
                                    let msg = String::from_utf8_lossy(&payload).to_string();
                                    let final_msg = if msg.is_empty() {
                                        format!("RPC method not found: {final_status:?}")
                                    } else {
                                        msg
                                    };
                                    sender.send_and_ignore(Err(RpcServiceError::Rpc(
                                        RpcServiceErrorPayload {
                                            code: RpcServiceErrorCode::NotFound,
                                            message: final_msg,
                                        },
                                    )));
                                    tracing::trace!(
                                        "[recv_fn for method: {}] Sent MethodNotFound error.",
                                        method_id
                                    );
                                }
                                Some(RpcResultStatus::Fail) => {
                                    sender.send_and_ignore(Err(RpcServiceError::Rpc(
                                        RpcServiceErrorPayload {
                                            code: RpcServiceErrorCode::Fail,
                                            message: "".into(),
                                        },
                                    )));
                                    tracing::trace!(
                                        "[recv_fn for method: {}] Sent Fail error.",
                                        method_id
                                    );
                                }
                                Some(RpcResultStatus::SystemError) => {
                                    let msg = String::from_utf8_lossy(&payload).to_string();
                                    let final_msg = if msg.is_empty() {
                                        format!("RPC failed with status: {final_status:?}")
                                    } else {
                                        msg
                                    };
                                    sender.send_and_ignore(Err(RpcServiceError::Rpc(
                                        RpcServiceErrorPayload {
                                            code: RpcServiceErrorCode::System,
                                            message: final_msg,
                                        },
                                    )));
                                    tracing::trace!(
                                        "[recv_fn for method: {method_id}] Sent SystemError.",
                                    );
                                }
                                _ => {
                                    tracing::trace!(
                                        "[recv_fn for method: {method_id}] Unexpected final status: {final_status:?}. Closing channel.",
                                    );
                                }
                            }
                        }
                        *tx_lock_guard = None;
                        tracing::trace!(
                            "[recv_fn for method: {}] DynamicSender dropped/channel closed on End event.",
                            method_id
                        );
                    }
                    RpcStreamEvent::Error {
                        frame_decode_error, ..
                    } => {
                        tracing::error!(
                            "[recv_fn for method: {}] Received Error event: {:?}",
                            method_id,
                            frame_decode_error
                        );
                        let error_to_send = RpcServiceError::Transport(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            frame_decode_error.to_string(),
                        ));
                        let mut temp_ready_tx_option = mem::take(&mut *ready_tx_lock_guard);
                        if let Some(tx_sender) = temp_ready_tx_option.take() {
                            let _ = tx_sender
                                .send(Err(io::Error::other(frame_decode_error.to_string())));
                            tracing::trace!(
                                "[recv_fn for method: {}] Sent error to readiness channel.",
                                method_id
                            );
                        }
                        let mut temp_tx_option = mem::take(&mut *tx_lock_guard);
                        if let Some(mut sender) = temp_tx_option.take() {
                            sender.send_and_ignore(Err(error_to_send));
                            tracing::trace!(
                                "[recv_fn for method: {}] Sent Transport error to DynamicSender and dropped it.",
                                method_id
                            );
                        } else {
                            tracing::trace!(
                                "[recv_fn for method: {}] DynamicSender already gone, cannot send Transport error.",
                                method_id
                            );
                        }
                        tracing::trace!(
                            "[recv_fn for method: {}] DynamicSender dropped/channel closed on Error event.",
                            method_id
                        );
                    }
                }
            })
        };

        let encoder: RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>;

        {
            let dispatcher_arc_clone = self.get_dispatcher();
            let mut dispatcher_guard = dispatcher_arc_clone.lock().await;

            tracing::debug!(
                "Registering call with dispatcher for method ID: {}.",
                request.rpc_method_id
            );

            encoder = dispatcher_guard
                .call(
                    request,
                    DEFAULT_SERVICE_MAX_CHUNK_SIZE,
                    send_fn,
                    Some(recv_fn),
                    false,
                )
                .map_err(|e| {
                    tracing::error!("Dispatcher.call failed: {e:?}");
                    RpcServiceError::Transport(io::Error::other(format!("{e:?}")))
                })?;

            tracing::trace!("`Dispatcher.call` returned encoder.");
        }

        // Signal readiness immediately — the call has been initiated.
        // For finalized (prebuffered) calls the response arrives later;
        // for non-finalized (streaming) calls the caller can now write
        // chunks and finalize without waiting for the response header.
        {
            let mut ready_tx_guard = ready_tx_arc.lock().unwrap();
            if let Some(tx_sender) = ready_tx_guard.take() {
                let _ = tx_sender.send(Ok(()));
            }
        }

        match ready_rx.await {
            Ok(Ok(())) => {
                tracing::trace!("Readiness signal received. Returning encoder and receiver.");
                Ok((encoder, rx))
            }
            Ok(Err(err)) => {
                tracing::trace!("Readiness signal received with error: {:?}", err);
                Err(RpcServiceError::Transport(err))
            }
            Err(_) => {
                tracing::error!("Readiness channel closed prematurely.");
                Err(RpcServiceError::Transport(io::Error::other(
                    "RPC setup channel closed prematurely",
                )))
            }
        }
    }

    #[instrument(skip(self, request, decode))]
    async fn call_rpc_buffered<T, F>(
        &self,
        request: RpcRequest,
        decode: F,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            Result<T, RpcServiceError>,
        ),
        RpcServiceError,
    >
    where
        T: Send + 'static,
        F: Fn(&[u8]) -> T + Send + Sync + 'static,
    {
        tracing::debug!("Starting for method ID: {}", request.rpc_method_id);
        let (encoder, mut stream) = self
            .call_rpc_streaming(request, DynamicChannelType::Unbounded)
            .await?;
        tracing::debug!("call_rpc_streaming returned. Entering stream consumption loop.");

        let mut success_buf = Vec::new();
        let mut err: Option<RpcServiceError> = None;

        while let Some(result) = stream.next().await {
            tracing::trace!("Stream yielded result: {:?}", result);
            match result {
                Ok(chunk) => {
                    success_buf.extend_from_slice(&chunk);
                    tracing::trace!("Added {} bytes to success buffer.", chunk.len());
                }
                Err(e) => {
                    tracing::trace!("Stream yielded error: {:?}", e);
                    err = Some(e);
                    break;
                }
            }
        }
        tracing::debug!("Stream consumption loop finished");

        if let Some(rpc_service_error) = err {
            tracing::error!("Returning with error from stream: {:?}", rpc_service_error);
            Ok((encoder, Err(rpc_service_error)))
        } else {
            tracing::debug!("Returning with success from stream.");
            Ok((encoder, Ok(decode(&success_buf))))
        }
    }

    async fn set_state_change_handler(
        &self,
        handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    );
}
