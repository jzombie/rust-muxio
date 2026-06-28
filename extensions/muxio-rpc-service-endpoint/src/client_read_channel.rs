use crate::{RpcServiceEndpoint, RpcServiceEndpointInterface};
use bytes::Bytes;
use muxio::rpc::RpcDispatcher;
use std::sync::{Arc, Weak};
use tokio::sync::Mutex;

/// Trait that a client transport must implement to use [`spawn_client_read_loop`].
#[async_trait::async_trait]
pub trait ClientReadTarget: Send + Sync + 'static {
    fn dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>>;
    fn endpoint(&self) -> Arc<RpcServiceEndpoint<()>>;
    async fn shutdown(&self);
}

/// Spawn a background task that reads bytes from `rx`, feeds them through
/// the endpoint, and calls [`ClientReadTarget::shutdown`] on EOF / error.
///
/// `rx` should yield only the payload bytes of incoming messages — any
/// transport-level filtering (ping/pong, close frames, etc.) must be handled
/// before passing to this function.
///
/// `emit` is called with response bytes that should be sent back over the
/// transport (e.g. `|bytes| tx.send(WsMessage::Binary(bytes.into()))`).
pub fn spawn_client_read_loop<C, S, E>(
    weak_client: Weak<C>,
    mut rx: S,
    emit: E,
) -> tokio::task::JoinHandle<()>
where
    C: ClientReadTarget,
    S: futures::Stream<Item = Bytes> + Unpin + Send + 'static,
    E: Fn(Vec<u8>) + Clone + Send + Sync + 'static,
{
    use futures::StreamExt;

    tokio::spawn(async move {
        while let Some(bytes) = rx.next().await {
            let Some(client) = weak_client.upgrade() else {
                break;
            };
            let dispatcher_arc = client.dispatcher();
            let mut dispatcher = dispatcher_arc.lock().await;
            let on_emit = emit.clone();
            let endpoint = client.endpoint();
            let _ = endpoint
                .read_bytes(&mut dispatcher, (), &bytes, move |chunk: &[u8]| {
                    on_emit(chunk.to_vec())
                })
                .await;
        }

        if let Some(client) = weak_client.upgrade() {
            tokio::spawn(async move {
                client.shutdown().await;
            });
        }
    })
}
