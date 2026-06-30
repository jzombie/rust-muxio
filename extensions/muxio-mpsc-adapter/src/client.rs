use std::sync::{Arc, Mutex};

use muxio_core::rpc::{
    RpcRequest,
    rpc_internals::{
        RpcStreamEvent,
        rpc_trait::{RpcEmit, RpcResponseHandler},
    },
};
use muxio_rpc_service::{
    error::RpcServiceError,
    constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE,
};
use muxio_rpc_service_caller::RpcServiceCallerInterface;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

#[async_trait::async_trait]
pub trait ChannelCallerExt: RpcServiceCallerInterface {
    /// Open a streaming channel for the given `method_id`.
    ///
    /// Returns a `(writer, reader)` pair:
    /// - `writer` — an unbounded sender; write request chunks here.
    /// - `reader` — an unbounded receiver yielding response chunks.
    ///
    /// `buffer_size` controls the response channel capacity. Pass `0` for
    /// unbounded (recommended for bursty streams like PTY output).
    /// The request writer is always unbounded so user writes never block.
    async fn open_channel(
        &self,
        method_id: u64,
        buffer_size: usize,
    ) -> Result<
        (
            UnboundedSender<Vec<u8>>,
            UnboundedReceiver<Result<Vec<u8>, RpcServiceError>>,
        ),
        RpcServiceError,
    > {
        if !self.is_connected() {
            return Err(RpcServiceError::Transport(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "RPC call attempted on a disconnected client.",
            )));
        }

        let (req_tx, mut req_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (resp_tx, resp_rx) = mpsc::unbounded_channel::<Result<Vec<u8>, RpcServiceError>>();
        let _ = buffer_size; // reserved for future use

        let request = RpcRequest {
            rpc_method_id: method_id,
            rpc_param_bytes: None,
            rpc_prebuffered_payload_bytes: None,
            is_finalized: false,
        };

        let on_emit = self.get_emit_fn();
        let send_fn: Box<dyn RpcEmit + Send + Sync> = Box::new(move |chunk: &[u8]| {
            on_emit(chunk.to_vec());
        });

        // Wrap the response sender in Arc<Mutex<Option<>>> so the closure
        // can take ownership and drop it on End/Error, closing the receiver.
        let resp_tx_holder = Arc::new(Mutex::new(Some(resp_tx)));
        let resp_tx_for_handler = resp_tx_holder.clone();
        let recv_fn: Box<dyn RpcResponseHandler + Send + 'static> =
            Box::new(move |event: RpcStreamEvent| {
                let mut guard = match resp_tx_for_handler.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                let tx = match guard.as_ref() {
                    Some(tx) => tx.clone(),
                    None => return,
                };
                match event {
                    RpcStreamEvent::PayloadChunk { bytes, .. } => {
                        let _ = tx.send(Ok(bytes));
                    }
                    RpcStreamEvent::End { .. } | RpcStreamEvent::Error { .. } => {
                        *guard = None;
                    }
                    _ => {}
                }
            });

        let dispatcher_arc = self.get_dispatcher();
        let mut dispatcher = dispatcher_arc.lock().await;

        let mut encoder = dispatcher
            .call(
                request,
                DEFAULT_SERVICE_MAX_CHUNK_SIZE,
                send_fn,
                Some(recv_fn),
                false,
            )
            .map_err(|e| {
                RpcServiceError::Transport(std::io::Error::other(format!("{e:?}")))
            })?;

        // Flush the header immediately so the remote sees the call start
        encoder.flush().map_err(|e| {
            RpcServiceError::Transport(std::io::Error::other(format!("{e:?}")))
        })?;

        drop(dispatcher);

        // Spawn a task that reads request bytes from the channel and writes
        // them through the encoder. When the sender is dropped, end the stream.
        tokio::spawn(async move {
            while let Some(bytes) = req_rx.recv().await {
                if encoder.write_bytes(&bytes).is_err() {
                    break;
                }
                if encoder.flush().is_err() {
                    break;
                }
            }
            let _ = encoder.end_stream();
        });

        Ok((req_tx, resp_rx))
    }
}

impl<T: RpcServiceCallerInterface> ChannelCallerExt for T {}
