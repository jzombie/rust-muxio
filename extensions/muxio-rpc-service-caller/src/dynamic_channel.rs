use futures::{
    Stream,
    channel::mpsc::{Receiver, Sender, UnboundedReceiver, UnboundedSender},
    pin_mut,
    task::{Context, Poll},
};
use muxio_rpc_service::error::RpcServiceError;
use std::pin::Pin;
use tracing::{self, instrument};

/// Defines the type of channel to be used for an RPC call's response stream.
#[derive(Debug, PartialEq)]
pub enum DynamicChannelType {
    Bounded,
    Unbounded,
}

// --- Enums and Implementations for Dynamic Channels ---

/// An enum to hold either a bounded or unbounded sender, unifying their interfaces.
pub enum DynamicSender {
    Bounded(Sender<Result<Vec<u8>, RpcServiceError>>),
    Unbounded(UnboundedSender<Result<Vec<u8>, RpcServiceError>>),
}

impl DynamicSender {
    /// A unified, non-blocking send method that preserves the original code's
    /// behavior of ignoring send errors (which typically only happen if the
    /// receiver has been dropped).
    #[instrument(skip(self, item))]
    pub fn send_and_ignore(&mut self, item: Result<Vec<u8>, RpcServiceError>) {
        match self {
            DynamicSender::Bounded(s) => {
                // For a bounded channel, try_send can fail if full or disconnected.
                let res = s.try_send(item);
                tracing::trace!("Bounded send result: {:?}", res);
            }
            DynamicSender::Unbounded(s) => {
                // For an unbounded channel, send can only fail if disconnected.
                let res = s.unbounded_send(item);
                tracing::trace!("Unbounded send result: {:?}", res);
            }
        }
    }
}

/// An enum to hold either a bounded or unbounded receiver.
pub enum DynamicReceiver {
    Bounded(Receiver<Result<Vec<u8>, RpcServiceError>>),
    Unbounded(UnboundedReceiver<Result<Vec<u8>, RpcServiceError>>),
}

/// Implement the `Stream` trait so our enum can be seamlessly used by consumers
/// like `while let Some(...) = stream.next().await`.
impl Stream for DynamicReceiver {
    type Item = Result<Vec<u8>, RpcServiceError>;

    #[instrument(skip(self, cx))]
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let poll_result = match self.get_mut() {
            DynamicReceiver::Bounded(r) => {
                let stream = r;
                pin_mut!(stream);
                stream.poll_next(cx)
            }
            DynamicReceiver::Unbounded(r) => {
                let stream = r;
                pin_mut!(stream);
                stream.poll_next(cx)
            }
        };
        tracing::trace!("Poll result: {:?}", poll_result);
        poll_result
    }
}
