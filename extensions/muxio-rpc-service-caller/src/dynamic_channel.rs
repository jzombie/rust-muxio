use crate::error::RpcCallerError;
use futures::{
    Stream,
    channel::mpsc::{Receiver, Sender, UnboundedReceiver, UnboundedSender},
    pin_mut,
    task::{Context, Poll},
};

use std::pin::Pin;

/// Defines the type of channel to be used for an RPC call's response stream.
#[derive(PartialEq)]
pub enum DynamicChannelType {
    Bounded,
    Unbounded,
}

// --- START: New Enums and Implementations for Dynamic Channels ---

/// An enum to hold either a bounded or unbounded sender, unifying their interfaces.
pub enum DynamicSender {
    Bounded(Sender<Result<Vec<u8>, RpcCallerError>>),
    Unbounded(UnboundedSender<Result<Vec<u8>, RpcCallerError>>),
}

impl DynamicSender {
    /// A unified, non-blocking send method that preserves the original code's
    /// behavior of ignoring send errors (which typically only happen if the
    /// receiver has been dropped).
    pub fn send_and_ignore(&mut self, item: Result<Vec<u8>, RpcCallerError>) {
        match self {
            DynamicSender::Bounded(s) => {
                // For a bounded channel, try_send can fail if full or disconnected.
                let _ = s.try_send(item);
            }
            DynamicSender::Unbounded(s) => {
                // For an unbounded channel, send can only fail if disconnected.
                let _ = s.unbounded_send(item);
            }
        }
    }
}

/// An enum to hold either a bounded or unbounded receiver.
pub enum DynamicReceiver {
    Bounded(Receiver<Result<Vec<u8>, RpcCallerError>>),
    Unbounded(UnboundedReceiver<Result<Vec<u8>, RpcCallerError>>),
}

/// Implement the `Stream` trait so our enum can be seamlessly used by consumers
/// like `while let Some(...) = stream.next().await`.
impl Stream for DynamicReceiver {
    type Item = Result<Vec<u8>, RpcCallerError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.get_mut() {
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
        }
    }
}
