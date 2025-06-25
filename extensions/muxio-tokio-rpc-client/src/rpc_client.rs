use futures_util::{SinkExt, StreamExt};
use muxio::rpc::RpcDispatcher;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, TransportState};
use std::io;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc as tokio_mpsc};
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

// The struct now includes the state change handler.
pub struct RpcClient {
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    tx: tokio_mpsc::UnboundedSender<WsMessage>,
    state_change_handler: Arc<Mutex<Option<Box<dyn Fn(TransportState) + Send + Sync>>>>,
}

impl RpcClient {
    pub async fn new(websocket_address: &str) -> Result<RpcClient, io::Error> {
        // --- 1. Initialize shared state, including the new handler ---
        let state_change_handler: Arc<Mutex<Option<Box<dyn Fn(TransportState) + Send + Sync>>>> =
            Arc::new(Mutex::new(None));

        // --- 2. Attempt to connect ---
        let (ws_stream, _) = connect_async(websocket_address)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;

        let (mut sender, mut receiver) = ws_stream.split();
        let (tx, mut rx) = tokio_mpsc::unbounded_channel::<WsMessage>();
        let (recv_tx, mut recv_rx) =
            tokio_mpsc::unbounded_channel::<Option<Result<WsMessage, WsError>>>();

        let dispatcher = Arc::new(Mutex::new(RpcDispatcher::new()));

        // --- 3. Spawn tasks to manage the connection lifecycle ---

        // Clone Arcs for the new tasks.
        let state_handler_clone_recv = state_change_handler.clone();
        let state_handler_clone_send = state_change_handler.clone();

        // Receive loop: Detects disconnection from the server side.
        tokio::spawn(async move {
            while let Some(msg) = receiver.next().await {
                let done = msg.is_err();
                if let Err(e) = recv_tx.send(Some(msg)) {
                    tracing::error!(
                        "DROPPED WEBSOCKET CHUNK: Client channel is full. Error: {}",
                        e
                    );
                }
                if done {
                    break;
                }
            }
            // --- 4. Signal disconnection when the receive stream ends ---
            if let Some(handler) = state_handler_clone_recv.lock().await.as_ref() {
                handler(TransportState::Disconnected);
            }
            let _ = recv_tx.send(None);
        });

        // Send loop: Detects disconnection if sends fail.
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if sender.send(msg).await.is_err() {
                    // --- 5. Signal disconnection when a send fails ---
                    if let Some(handler) = state_handler_clone_send.lock().await.as_ref() {
                        handler(TransportState::Disconnected);
                    }
                    break;
                }
            }
        });

        // Handle incoming binary messages.
        let dispatcher_handle = dispatcher.clone();
        tokio::spawn(async move {
            while let Some(Some(Ok(WsMessage::Binary(bytes)))) = recv_rx.recv().await {
                dispatcher_handle.lock().await.read_bytes(&bytes).ok();
            }
        });

        // --- 6. Return the fully constructed client ---
        Ok(RpcClient {
            dispatcher,
            tx,
            state_change_handler,
        })
    }
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcClient {
    type DispatcherLock = Mutex<RpcDispatcher<'static>>;

    fn get_dispatcher(&self) -> Arc<Self::DispatcherLock> {
        self.dispatcher.clone()
    }

    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        Arc::new({
            let tx = self.tx.clone();
            move |chunk: Vec<u8>| {
                let _ = tx.send(WsMessage::Binary(chunk.into()));
            }
        })
    }

    // --- 7. Implementation of the new handler registration method ---
    /// Sets a callback that will be invoked with the current `TransportState`
    /// whenever the WebSocket connection status changes.
    fn set_state_change_handler(&self, handler: impl Fn(TransportState) + Send + Sync + 'static) {
        let mut state_handler = self
            .state_change_handler
            .try_lock()
            .expect("Mutex should not be locked here");
        *state_handler = Some(Box::new(handler));

        // Immediately invoke the handler with the current "Connected" state.
        if let Some(h) = state_handler.as_ref() {
            h(TransportState::Connected);
        }
    }
}
