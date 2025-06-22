use crate::{
    dynamic_channel::{DynamicReceiver, DynamicSender},
    error::RpcCallerError,
    with_dispatcher_trait::WithDispatcher,
};
use futures::{StreamExt, channel::mpsc, channel::oneshot};
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
#[async_trait::async_trait]
pub trait RpcServiceCallerInterface: Send + Sync {
    type DispatcherLock: WithDispatcher;

    fn get_dispatcher(&self) -> Arc<Self::DispatcherLock>;
    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync>;

    /// Performs a streaming RPC call, yielding a stream of success payloads or a terminal error.
    async fn call_rpc_streaming(
        &self,
        request: RpcRequest,
        use_unbounded_channel: bool,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            DynamicReceiver,
        ),
        io::Error,
    > {
        // Dynamically create the chosen channel type based on the flag
        let (tx, rx) = if use_unbounded_channel {
            let (sender, receiver) = mpsc::unbounded();
            (
                DynamicSender::Unbounded(sender),
                DynamicReceiver::Unbounded(receiver),
            )
        } else {
            // Correctly uses the constant when creating the bounded channel
            let (sender, receiver) = mpsc::channel(DEFAULT_RPC_STREAM_CHANNEL_BUFFER_SIZE);
            (
                DynamicSender::Bounded(sender),
                DynamicReceiver::Bounded(receiver),
            )
        };

        let tx = Arc::new(Mutex::new(Some(tx)));

        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), io::Error>>();
        let ready_tx = Arc::new(Mutex::new(Some(ready_tx)));

        let send_fn: Box<dyn RpcEmit + Send + Sync> = Box::new({
            let on_emit = self.get_emit_fn();
            move |chunk: &[u8]| {
                on_emit(chunk.to_vec());
            }
        });

        let recv_fn: Box<dyn FnMut(RpcStreamEvent) + Send + 'static> = {
            let status = Arc::new(Mutex::new(None::<RpcResultStatus>));
            let error_buffer = Arc::new(Mutex::new(Vec::new()));

            Box::new(move |evt| {
                let mut tx_lock = tx.lock().expect("tx mutex poisoned");
                match evt {
                    RpcStreamEvent::Header { rpc_header, .. } => {
                        let result_status = rpc_header
                            .rpc_metadata_bytes
                            .first()
                            .copied()
                            .and_then(|b| RpcResultStatus::try_from(b).ok())
                            .unwrap_or(RpcResultStatus::Success);
                        *status.lock().expect("status mutex poisoned") = Some(result_status);
                        if let Some(tx) = ready_tx.lock().expect("ready_tx mutex poisoned").take() {
                            let _ = tx.send(Ok(()));
                        }
                    }
                    RpcStreamEvent::PayloadChunk { bytes, .. } => {
                        let current_status = status.lock().expect("status mutex poisoned");
                        match *current_status {
                            Some(RpcResultStatus::Success) => {
                                if let Some(sender) = tx_lock.as_mut() {
                                    // Use the unified send method from our new enum
                                    sender.send_and_ignore(Ok(bytes));
                                }
                            }
                            Some(_) => {
                                error_buffer
                                    .lock()
                                    .expect("error buffer mutex poisoned")
                                    .extend(bytes);
                            }
                            None => {}
                        }
                    }
                    RpcStreamEvent::End { .. } => {
                        let final_status = status.lock().expect("status mutex poisoned").take();
                        let payload = std::mem::take(
                            &mut *error_buffer.lock().expect("error buffer mutex poisoned"),
                        );
                        if let Some(sender) = tx_lock.as_mut() {
                            match final_status {
                                Some(RpcResultStatus::Fail) => {
                                    sender.send_and_ignore(Err(RpcCallerError::RemoteError {
                                        payload,
                                    }));
                                }
                                Some(status @ RpcResultStatus::SystemError)
                                | Some(status @ RpcResultStatus::MethodNotFound) => {
                                    let msg = String::from_utf8_lossy(&payload).to_string();
                                    let final_msg = if msg.is_empty() {
                                        format!("RPC failed with status: {:?}", status)
                                    } else {
                                        msg
                                    };
                                    sender.send_and_ignore(Err(RpcCallerError::RemoteSystemError(
                                        final_msg,
                                    )));
                                }
                                _ => {}
                            }
                        }
                        *tx_lock = None;
                    }
                    _ => {}
                }
            })
        };

        // The rest of the function remains unchanged
        let encoder = self
            .get_dispatcher()
            .with_dispatcher(|d| {
                d.call(
                    request,
                    DEFAULT_SERVICE_MAX_CHUNK_SIZE,
                    send_fn,
                    Some(recv_fn),
                    false,
                )
            })
            .await
            .map_err(|e| io::Error::other(format!("{:?}", e)))?;

        match ready_rx.await {
            Ok(Ok(())) => Ok((encoder, rx)),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(io::Error::other("RPC setup channel closed prematurely")),
        }
    }

    /// Performs a buffered RPC call that can resolve to a success value or a custom error.
    async fn call_rpc_buffered<T, F>(
        &self,
        request: RpcRequest,
        decode: F,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            Result<T, RpcCallerError>,
        ),
        io::Error,
    >
    where
        T: Send + 'static,
        F: Fn(&[u8]) -> T + Send + Sync + 'static,
    {
        // Defaults to using the safer, bounded channel.
        let (encoder, mut stream) = self.call_rpc_streaming(request, false).await?;

        let mut success_buf = Vec::new();
        let mut err: Option<RpcCallerError> = None;

        while let Some(result) = stream.next().await {
            match result {
                Ok(chunk) => {
                    success_buf.extend_from_slice(&chunk);
                }
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }

        if let Some(e) = err {
            Ok((encoder, Err(e)))
        } else {
            Ok((encoder, Ok(decode(&success_buf))))
        }
    }
}
