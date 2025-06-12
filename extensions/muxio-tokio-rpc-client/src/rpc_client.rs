use futures_util::{SinkExt, StreamExt};
use muxio::rpc::RpcDispatcher;
use muxio_rpc_service_caller::RpcServiceCallerInterface;
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

// This is the new, much simpler implementation block.
#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcClient {
    type DispatcherLock = Mutex<RpcDispatcher<'static>>;

    /// Provides the trait with access to this client's dispatcher.
    fn get_dispatcher(&self) -> Arc<Self::DispatcherLock> {
        self.dispatcher.clone()
    }

    /// Provides the trait with this client's specific method for sending bytes
    /// over its WebSocket connection.
    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        Arc::new({
            let tx = self.tx.clone();
            move |chunk: Vec<u8>| {
                let _ = tx.send(WsMessage::Binary(chunk.into()));
            }
        })
    }
}
