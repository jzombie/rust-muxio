use futures_util::{SinkExt, StreamExt};
use muxio::rpc::RpcDispatcher;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use std::fmt;
use std::io;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::mpsc as tokio_mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

type RpcTransportStateChangeHandler = Arc<Mutex<Option<Box<dyn Fn(RpcTransportState) + Send + Sync>>>>;

pub struct RpcClient {
    dispatcher: Arc<tokio::sync::Mutex<RpcDispatcher<'static>>>,
    tx: tokio_mpsc::UnboundedSender<WsMessage>,
    state_change_handler: RpcTransportStateChangeHandler,
    is_connected: Arc<AtomicBool>,
    // This field holds the handles to the background tasks.
    // When RpcClient is dropped, these handles are also dropped, aborting the tasks.
    _task_handles: Vec<JoinHandle<()>>,
}

impl fmt::Debug for RpcClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RpcClient")
            .field("dispatcher", &"Arc<Mutex<RpcDispatcher>>")
            .field("tx", &self.tx)
            .field("state_change_handler", &"Arc<Mutex<...>>")
            .field("is_connected", &self.is_connected.load(Ordering::Relaxed))
            .finish()
    }
}

// Implement Drop to ensure tasks are cleaned up and state is updated.
impl Drop for RpcClient {
    fn drop(&mut self) {
        // Abort all background tasks associated with this client.
        for handle in &self._task_handles {
            handle.abort();
        }
        // Atomically set the connection state to false and signal the change.
        if self.is_connected.swap(false, Ordering::SeqCst) {
            if let Ok(guard) = self.state_change_handler.lock() {
                if let Some(handler) = guard.as_ref() {
                    handler(RpcTransportState::Disconnected);
                }
            }
        }
    }
}

impl RpcClient {
    pub async fn new(websocket_address: &str) -> Result<RpcClient, io::Error> {
        let (ws_stream, _) = connect_async(websocket_address)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        let (app_tx, mut app_rx) = tokio_mpsc::unbounded_channel::<WsMessage>();
        let (ws_recv_tx, mut ws_recv_rx) =
            tokio_mpsc::unbounded_channel::<Option<Result<WsMessage, WsError>>>();

        let state_change_handler: RpcTransportStateChangeHandler = Arc::new(Mutex::new(None));

        let is_connected = Arc::new(AtomicBool::new(true));
        let dispatcher = Arc::new(tokio::sync::Mutex::new(RpcDispatcher::new()));

        let mut task_handles = Vec::new();

        // Clones for tasks
        let is_connected_recv = is_connected.clone();
        let state_handler_recv = state_change_handler.clone();
        let dispatcher_handle = dispatcher.clone();

        // Receive loop: Forwards messages from WebSocket to the dispatcher task.
        let recv_handle = tokio::spawn(async move {
            while let Some(msg) = ws_receiver.next().await {
                if ws_recv_tx.send(Some(msg)).is_err() {
                    break;
                }
            }
            if is_connected_recv.swap(false, Ordering::SeqCst) {
                if let Some(handler) = state_handler_recv.lock().unwrap().as_ref() {
                    handler(RpcTransportState::Disconnected);
                }
            }
            let _ = ws_recv_tx.send(None);
        });
        task_handles.push(recv_handle);

        // Send loop: Forwards messages from the application to the WebSocket.
        let send_handle = tokio::spawn(async move {
            while let Some(msg) = app_rx.recv().await {
                if ws_sender.send(msg).await.is_err() {
                    break;
                }
            }
        });
        task_handles.push(send_handle);

        // Dispatcher loop: Processes incoming messages from the receive loop.
        let dispatch_handle = tokio::spawn(async move {
            while let Some(Some(Ok(WsMessage::Binary(bytes)))) = ws_recv_rx.recv().await {
                dispatcher_handle.lock().await.read_bytes(&bytes).ok();
            }
        });
        task_handles.push(dispatch_handle);

        Ok(RpcClient {
            dispatcher,
            tx: app_tx,
            state_change_handler,
            is_connected,
            _task_handles: task_handles,
        })
    }
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcClient {
    type DispatcherLock = tokio::sync::Mutex<RpcDispatcher<'static>>;

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

    /// Sets a callback that will be invoked with the current `RpcTransportState`
    /// whenever the WebSocket connection status changes.
    fn set_state_change_handler(&self, handler: impl Fn(RpcTransportState) + Send + Sync + 'static) {
        let mut state_handler = self
            .state_change_handler
            .lock()
            .expect("Mutex should not be poisoned");
        *state_handler = Some(Box::new(handler));

        if self.is_connected.load(Ordering::SeqCst) {
            if let Some(h) = state_handler.as_ref() {
                h(RpcTransportState::Connected);
            }
        }
    }
}
