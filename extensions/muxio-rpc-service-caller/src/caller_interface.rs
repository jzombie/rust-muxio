use crate::{
    RpcTransportState,
    dynamic_channel::{DynamicChannelType, DynamicReceiver, DynamicSender},
};
use futures::{StreamExt, channel::mpsc, channel::oneshot};
use muxio::rpc::{
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
use std::io;
use std::mem;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use tokio::task; // ADD THIS IMPORT

#[async_trait::async_trait]
pub trait RpcServiceCallerInterface: Send + Sync {
    fn get_dispatcher(&self) -> Arc<TokioMutex<RpcDispatcher<'static>>>;
    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync>;

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
        println!(
            "[RpcServiceCallerInterface::call_rpc_streaming] Starting for method ID: {}",
            request.rpc_method_id
        );
        let (tx, rx) = match dynamic_channel_type {
            DynamicChannelType::Unbounded => {
                let (sender, receiver) = mpsc::unbounded();
                println!(
                    "[RpcServiceCallerInterface::call_rpc_streaming] Created Unbounded channel."
                );
                (
                    DynamicSender::Unbounded(sender),
                    DynamicReceiver::Unbounded(receiver),
                )
            }
            DynamicChannelType::Bounded => {
                let (sender, receiver) = mpsc::channel(DEFAULT_RPC_STREAM_CHANNEL_BUFFER_SIZE);
                println!(
                    "[RpcServiceCallerInterface::call_rpc_streaming] Created Bounded channel."
                );
                (
                    DynamicSender::Bounded(sender),
                    DynamicReceiver::Bounded(receiver),
                )
            }
        };

        let tx_arc = Arc::new(TokioMutex::new(Some(tx)));
        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), io::Error>>();
        let ready_tx_arc = Arc::new(TokioMutex::new(Some(ready_tx)));
        println!(
            "[RpcServiceCallerInterface::call_rpc_streaming] Oneshot channel for readiness created."
        );

        let send_fn: Box<dyn RpcEmit + Send + Sync> = Box::new({
            let on_emit = self.get_emit_fn();
            move |chunk: &[u8]| {
                on_emit(chunk.to_vec());
            }
        });

        let recv_fn: Box<dyn RpcResponseHandler + Send + 'static> = {
            let status = Arc::new(TokioMutex::new(None::<RpcResultStatus>));
            let error_buffer = Arc::new(TokioMutex::new(Vec::new()));
            let method_id = request.rpc_method_id;

            let tx_clone_for_recv_fn = tx_arc.clone();
            let ready_tx_clone_for_recv_fn = ready_tx_arc.clone();

            Box::new(move |evt| {
                // This closure is SYNCHRONOUS
                println!(
                    "[RpcServiceCallerInterface::recv_fn for method {}] Received event: {:?}",
                    method_id, evt
                );

                // FIX: Offload the blocking operations to a blocking task.
                // This is necessary because RpcResponseHandler is a synchronous trait,
                // but its implementation needs to acquire TokioMutexes.
                let tx_clone_inner = tx_clone_for_recv_fn.clone();
                let status_clone_inner = status.clone();
                let ready_tx_clone_inner = ready_tx_clone_for_recv_fn.clone();
                let error_buffer_clone_inner = error_buffer.clone();
                let evt_clone = evt; // RpcStreamEvent is likely Copy or Clone, ensure it's moved/cloned properly

                task::spawn_blocking(move || {
                    // <--- SPAWN BLOCKING HERE
                    // All mutex locking and inner logic now happens on a blocking thread.
                    let mut tx_lock_guard = tx_clone_inner.blocking_lock();
                    let mut status_lock_guard = status_clone_inner.blocking_lock();
                    let mut ready_tx_lock_guard = ready_tx_clone_inner.blocking_lock();
                    let mut error_buffer_lock_guard = error_buffer_clone_inner.blocking_lock();

                    // --- Your existing recv_fn logic goes here, operating on the guards ---
                    match evt_clone {
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
                                println!(
                                    "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) Sent readiness signal.",
                                    method_id
                                );
                            }
                        }
                        RpcStreamEvent::PayloadChunk { bytes, .. } => {
                            let bytes_len = bytes.len();
                            let mut current_status_option = mem::take(&mut *status_lock_guard);
                            match current_status_option.as_ref() {
                                Some(RpcResultStatus::Success) => {
                                    let mut temp_tx_option = mem::take(&mut *tx_lock_guard);
                                    if let Some(sender) = temp_tx_option.as_mut() {
                                        sender.send_and_ignore(Ok(bytes));
                                        println!(
                                            "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) Sent payload chunk ({} bytes) to DynamicSender.",
                                            method_id, bytes_len
                                        );
                                    }
                                    *tx_lock_guard = temp_tx_option;
                                }
                                Some(_) => {
                                    error_buffer_lock_guard.extend(bytes);
                                    println!(
                                        "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) Buffered error payload chunk ({} bytes).",
                                        method_id, bytes_len
                                    );
                                }
                                None => {
                                    println!(
                                        "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) Received payload before status. Buffering.",
                                        method_id
                                    );
                                    error_buffer_lock_guard.extend(bytes);
                                    println!(
                                        "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) Buffered payload chunk ({} bytes) before status.",
                                        method_id, bytes_len
                                    );
                                }
                            }
                            *status_lock_guard = current_status_option;
                        }
                        RpcStreamEvent::End { .. } => {
                            println!(
                                "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) Received End event.",
                                method_id
                            );
                            let final_status = mem::take(&mut *status_lock_guard);
                            let payload =
                                std::mem::replace(&mut *error_buffer_lock_guard, Vec::new());
                            let mut temp_tx_option = mem::take(&mut *tx_lock_guard);
                            if let Some(mut sender) = temp_tx_option.take() {
                                match final_status {
                                    Some(RpcResultStatus::MethodNotFound) => {
                                        let msg = String::from_utf8_lossy(&payload).to_string();
                                        let final_msg = if msg.is_empty() {
                                            format!("RPC method not found: {:?}", final_status)
                                        } else {
                                            msg
                                        };
                                        sender.send_and_ignore(Err(RpcServiceError::Rpc(
                                            RpcServiceErrorPayload {
                                                code: RpcServiceErrorCode::NotFound,
                                                message: final_msg,
                                            },
                                        )));
                                        println!(
                                            "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) Sent MethodNotFound error.",
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
                                        println!(
                                            "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) Sent Fail error.",
                                            method_id
                                        );
                                    }
                                    Some(RpcResultStatus::SystemError) => {
                                        let msg = String::from_utf8_lossy(&payload).to_string();
                                        let final_msg = if msg.is_empty() {
                                            format!("RPC failed with status: {:?}", final_status)
                                        } else {
                                            msg
                                        };
                                        sender.send_and_ignore(Err(RpcServiceError::Rpc(
                                            RpcServiceErrorPayload {
                                                code: RpcServiceErrorCode::System,
                                                message: final_msg,
                                            },
                                        )));
                                        println!(
                                            "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) Sent SystemError.",
                                            method_id
                                        );
                                    }
                                    _ => {
                                        println!(
                                            "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) Unexpected final status: {:?}. Closing channel.",
                                            method_id, final_status
                                        );
                                    }
                                }
                            }
                            *tx_lock_guard = None;
                            println!(
                                "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) DynamicSender dropped/channel closed on End event.",
                                method_id
                            );
                        }
                        RpcStreamEvent::Error {
                            frame_decode_error, ..
                        } => {
                            println!(
                                "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) Received Error event: {:?}",
                                method_id, frame_decode_error
                            );
                            let error_to_send = RpcServiceError::Transport(io::Error::new(
                                io::ErrorKind::ConnectionAborted,
                                frame_decode_error.to_string(),
                            ));
                            let mut temp_ready_tx_option = mem::take(&mut *ready_tx_lock_guard);
                            if let Some(tx_sender) = temp_ready_tx_option.take() {
                                let _ = tx_sender
                                    .send(Err(io::Error::other(frame_decode_error.to_string())));
                                println!(
                                    "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) Sent error to readiness channel.",
                                    method_id
                                );
                            }
                            let mut temp_tx_option = mem::take(&mut *tx_lock_guard);
                            if let Some(mut sender) = temp_tx_option.take() {
                                sender.send_and_ignore(Err(error_to_send));
                                println!(
                                    "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) Sent Transport error to DynamicSender and dropped it.",
                                    method_id
                                );
                            } else {
                                println!(
                                    "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) DynamicSender already gone, cannot send Transport error.",
                                    method_id
                                );
                            }
                            println!(
                                "[RpcServiceCallerInterface::recv_fn for method {}] (Blocking Task) DynamicSender dropped/channel closed on Error event.",
                                method_id
                            );
                        }
                    }
                }); // End of spawn_blocking
            }) // End of Box::new closure
        };

        let encoder; // Declare encoder here
        let rx_result: Result<
            (
                RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
                DynamicReceiver,
            ),
            RpcServiceError,
        >;

        {
            // NEW SCOPE BLOCK TO LIMIT MUTEX GUARD LIFETIME
            let dispatcher_arc_clone = self.get_dispatcher(); // Re-clone Arc if needed for safety, or ensure original lives
            let mut dispatcher_guard = dispatcher_arc_clone.lock().await; // Acquire lock

            println!(
                "[RpcServiceCallerInterface::call_rpc_streaming] Registering call with dispatcher for method ID: {}.",
                request.rpc_method_id
            );

            // The `call` method internally uses `send_fn` to emit the request bytes.
            let result_encoder = dispatcher_guard
                .call(
                    request,
                    DEFAULT_SERVICE_MAX_CHUNK_SIZE,
                    send_fn,
                    Some(recv_fn),
                    false,
                )
                .map_err(|e| {
                    println!(
                        "[RpcServiceCallerInterface::call_rpc_streaming] Dispatcher.call failed: {:?}",
                        e
                    );
                    io::Error::other(format!("{:?}", e))
                });

            // If dispatcher.call succeeded, unwrap the encoder and prepare the result.
            // If it failed, propagate the error.
            match result_encoder {
                Ok(enc) => {
                    encoder = enc;
                    // Prepare the result tuple to be returned AFTER the dispatcher_guard is dropped.
                    rx_result = Ok((encoder, rx));
                }
                Err(e) => {
                    rx_result = Err(RpcServiceError::Transport(e)); // Convert io::Error to RpcServiceError
                }
            }

            println!(
                "[RpcServiceCallerInterface::call_rpc_streaming] Dispatcher.call returned encoder."
            );
        } // <--- dispatcher_guard IS EXPLICITLY DROPPED HERE, RELEASING THE MUTEX!

        // Now, after the dispatcher lock is released, we can await the readiness signal.
        match ready_rx.await {
            Ok(Ok(())) => {
                println!(
                    "[RpcServiceCallerInterface::call_rpc_streaming] Readiness signal received. Returning encoder and receiver."
                );
                rx_result // Return the result prepared earlier
            }
            Ok(Err(err)) => {
                println!(
                    "[RpcServiceCallerInterface::call_rpc_streaming] Readiness signal received with error: {:?}",
                    err
                );
                Err(RpcServiceError::Transport(err))
            }
            Err(_) => {
                println!(
                    "[RpcServiceCallerInterface::call_rpc_streaming] Readiness channel closed prematurely."
                );
                Err(RpcServiceError::Transport(io::Error::other(
                    "RPC setup channel closed prematurely",
                )))
            }
        }
    }

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
        println!(
            "[RpcServiceCallerInterface::call_rpc_buffered] Starting for method ID: {}",
            request.rpc_method_id
        );
        let (encoder, mut stream) = self
            .call_rpc_streaming(request, DynamicChannelType::Unbounded)
            .await?;
        println!(
            "[RpcServiceCallerInterface::call_rpc_buffered] call_rpc_streaming returned. Entering stream consumption loop."
        );

        let mut success_buf = Vec::new();
        let mut err: Option<RpcServiceError> = None;

        while let Some(result) = stream.next().await {
            println!(
                "[RpcServiceCallerInterface::call_rpc_buffered] Stream yielded result: {:?}",
                result
            );
            match result {
                Ok(chunk) => {
                    success_buf.extend_from_slice(&chunk);
                    println!(
                        "[RpcServiceCallerInterface::call_rpc_buffered] Added {} bytes to success buffer.",
                        chunk.len()
                    );
                }
                Err(e) => {
                    println!(
                        "[RpcServiceCallerInterface::call_rpc_buffered] Stream yielded error: {:?}",
                        e
                    );
                    err = Some(e);
                    break;
                }
            }
        }
        println!(
            "[RpcServiceCallerInterface::call_rpc_buffered] Stream consumption loop finished. Error: {:?}",
            err
        );

        if let Some(e) = err {
            println!(
                "[RpcServiceCallerInterface::call_rpc_buffered] Returning with error from stream: {:?}",
                e
            );
            Ok((encoder, Err(e)))
        } else {
            println!(
                "[RpcServiceCallerInterface::call_rpc_buffered] Returning with success from stream."
            );
            Ok((encoder, Ok(decode(&success_buf))))
        }
    }

    async fn set_state_change_handler(
        &self,
        handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    );
}
