use futures::channel::mpsc;
use futures_util::{SinkExt, StreamExt};
use muxio::rpc::{
    RpcDispatcher,
    rpc_internals::{RpcStreamEncoder, rpc_trait::RpcEmit},
};
use muxio_rpc_service_caller::{RpcServiceCaller, RpcServiceCallerInterface};
use std::io;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc as tokio_mpsc};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

pub struct RpcClient {
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    tx: tokio_mpsc::UnboundedSender<WsMessage>,
}

impl RpcClient {
    pub async fn new(websocket_address: &str) -> RpcClient {
        let (ws_stream, _) = connect_async(websocket_address)
            .await
            // TODO: Don't use expect or unwrap
            .expect("Failed to connect");
        let (mut sender, mut receiver) = ws_stream.split();

        let (tx, mut rx) = tokio_mpsc::unbounded_channel::<WsMessage>();
        let (recv_tx, mut recv_rx) = tokio_mpsc::unbounded_channel::<
            Option<Result<WsMessage, tokio_tungstenite::tungstenite::Error>>,
        >();

        let dispatcher = Arc::new(Mutex::new(RpcDispatcher::new()));

        // Receive loop
        tokio::spawn(async move {
            while let Some(msg) = receiver.next().await {
                let done = msg.is_err();
                let _ = recv_tx.send(Some(msg));
                if done {
                    break;
                }
            }
            let _ = recv_tx.send(None);
        });

        // Send loop
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if sender.send(msg).await.is_err() {
                    break;
                }
            }
        });

        // Handle incoming
        let dispatcher_handle = dispatcher.clone();
        tokio::spawn(async move {
            while let Some(Some(Ok(WsMessage::Binary(bytes)))) = recv_rx.recv().await {
                dispatcher_handle.lock().await.read_bytes(&bytes).ok();
            }
        });

        RpcClient { dispatcher, tx }
    }
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcClient {
    type DispatcherMutex<T> = Mutex<T>;

    fn get_dispatcher(&self) -> Arc<Self::DispatcherMutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    async fn call_rpc_streaming(
        &self,
        method_id: u64,
        payload: &[u8],
        is_finalized: bool,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            mpsc::Receiver<Vec<u8>>,
        ),
        io::Error,
    > {
        // Create the emit callback for the generic function.
        // It captures the tokio MPSC sender for the websocket.
        let emit_fn: Arc<dyn Fn(Vec<u8>) + Send + Sync> = Arc::new({
            let tx = self.tx.clone();
            move |chunk: Vec<u8>| {
                // The generic handler gives us a Vec<u8>, which we wrap in a WsMessage.
                let _ = tx.send(WsMessage::Binary(chunk.into()));
            }
        });

        // Delegate directly to the generic function.
        // The dispatcher (Arc<tokio::sync::Mutex<...>>) works because we implemented
        // the WithDispatcher trait for it in the other crate.
        RpcServiceCaller::call_rpc_streaming(
            self.get_dispatcher(),
            emit_fn,
            method_id,
            payload,
            is_finalized,
        )
        .await
    }

    async fn call_rpc_buffered<T, F>(
        &self,
        method_id: u64,
        payload: &[u8],
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
        // Delegate directly to the generic buffered helper
        RpcServiceCaller::call_rpc_buffered(self, method_id, payload, decode, is_finalized).await
    }
}
