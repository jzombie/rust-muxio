use std::sync::{Arc, Mutex};

use muxio_core::rpc::rpc_internals::RpcStreamEvent;
use muxio_rpc_service_endpoint::{
    RpcServiceEndpointInterface, StreamResponder, error::RpcServiceEndpointError,
};
use tokio::sync::mpsc;

/// Trait abstracting over bounded and unbounded mpsc senders.
///
/// Both `mpsc::Sender<T>` and `mpsc::UnboundedSender<T>` implement this
/// trait, so callers can choose the channel flavour.
pub trait MpscSender: Clone + Send + Sync + 'static {
    fn try_send_bytes(&self, bytes: Vec<u8>);
}

impl MpscSender for mpsc::Sender<Vec<u8>> {
    fn try_send_bytes(&self, bytes: Vec<u8>) {
        let _ = self.try_send(bytes);
    }
}

impl MpscSender for mpsc::UnboundedSender<Vec<u8>> {
    fn try_send_bytes(&self, bytes: Vec<u8>) {
        let _ = self.send(bytes);
    }
}

#[async_trait::async_trait]
pub trait ChannelEndpointExt<C>: RpcServiceEndpointInterface<C>
where
    C: Send + Sync + Clone + 'static,
{
    /// Register a streaming handler for `method_id` that forwards incoming
    /// payload chunks through an mpsc sender.
    ///
    /// The sender is dropped on `End` / `Error` events, which closes the
    /// corresponding receiver.
    ///
    /// # When to use this vs. raw `register_stream_handler`
    ///
    /// | `register_channel_handler` | raw `register_stream_handler` |
    /// |---|---|
    /// | Each stream gets the same sender. On `End` the sender is dropped and the channel closes. | You control the sender lifecycle; `End` is a no-op unless you choose otherwise. |
    /// | Best for **per-stream work queues**: one request → one stream → one channel → receiver exits cleanly. | Best for **shared sinks** that outlive individual connections: PTY input, broadcast fan-out, persistent subscriptions. |
    ///
    /// If a reconnect must keep working (the channel survives first client's
    /// disconnect), use raw `register_stream_handler` and keep the sender
    /// alive by ignoring `End`/`Error`.
    async fn register_channel_handler<S>(
        &self,
        method_id: u64,
        tx: S,
    ) -> Result<(), RpcServiceEndpointError>
    where
        S: MpscSender,
    {
        let tx = Arc::new(Mutex::new(Some(tx)));
        self.register_stream_handler(
            method_id,
            move |event, _responder: StreamResponder, _ctx: C| {
                let mut guard = match tx.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                match event {
                    RpcStreamEvent::PayloadChunk { bytes, .. } => {
                        if let Some(ref inner) = *guard {
                            inner.try_send_bytes(bytes);
                        }
                    }
                    RpcStreamEvent::End { .. } | RpcStreamEvent::Error { .. } => {
                        *guard = None;
                    }
                    _ => {}
                }
            },
        )
        .await
    }
}

impl<T, C> ChannelEndpointExt<C> for T
where
    T: RpcServiceEndpointInterface<C>,
    C: Send + Sync + Clone + 'static,
{
}
