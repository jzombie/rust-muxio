use futures_util::{SinkExt, StreamExt};
use muxio::rpc::RpcDispatcher;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use std::fmt;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::mpsc as tokio_mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

type RpcTransportStateChangeHandler =
    Arc<Mutex<Option<Box<dyn Fn(RpcTransportState) + Send + Sync>>>>;

pub struct RpcClient {
    dispatcher: Arc<tokio::sync::Mutex<RpcDispatcher<'static>>>,
    tx: tokio_mpsc::UnboundedSender<WsMessage>,
    state_change_handler: RpcTransportStateChangeHandler,
    is_connected: Arc<AtomicBool>,
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

impl Drop for RpcClient {
    fn drop(&mut self) {
        for handle in &self._task_handles {
            handle.abort();
        }
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
    /// Creates a new RPC client and connects to a WebSocket server.
    ///
    /// The `host` can be either an IP address (v4 or v6) or a hostname that
    /// will be resolved via DNS.
    pub async fn new(host: &str, port: u16) -> Result<RpcClient, io::Error> {
        // Construct the URL.
        // This handles proper IPv6 bracket formatting `[::1]` for IP literals,
        // while passing hostnames through for DNS resolution by the network stack.
        let websocket_url = match host.parse::<IpAddr>() {
            // It's a valid IP address literal.
            Ok(ip) => {
                let socket_addr = SocketAddr::new(ip, port);
                format!("ws://{socket_addr}/ws")
            }
            // It's not an IP address, so assume it's a hostname.
            Err(_) => {
                format!("ws://{host}:{port}/ws")
            }
        };

        let (ws_stream, _) = connect_async(websocket_url.to_string())
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
        let is_connected_recv = is_connected.clone();
        let state_handler_recv = state_change_handler.clone();
        let dispatcher_handle = dispatcher.clone();
        let tx_for_handler = app_tx.clone();

        // Receive loop: Forwards all messages from the WebSocket to the handler task.
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

        // Message handler loop: Processes all incoming messages.
        let dispatch_handle = tokio::spawn(async move {
            while let Some(Some(msg_result)) = ws_recv_rx.recv().await {
                match msg_result {
                    Ok(WsMessage::Binary(bytes)) => {
                        // Forward binary data to the RPC dispatcher.
                        dispatcher_handle.lock().await.read_bytes(&bytes).ok();
                    }
                    Ok(WsMessage::Ping(data)) => {
                        // Received a Ping from the server, respond with a Pong.
                        let _ = tx_for_handler.send(WsMessage::Pong(data));
                    }
                    Ok(WsMessage::Close(_)) => {
                        // The connection is closing, break the loop.
                        // The main receive loop will handle the disconnect signal.
                        break;
                    }
                    Err(e) => {
                        // An error occurred on the WebSocket stream.
                        // The main receive loop will handle the disconnect signal.
                        tracing::error!("WebSocket error: {}", e);
                        break;
                    }
                    // Ignore other message types like Pong from server, Text, etc.
                    _ => {}
                }
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
    fn set_state_change_handler(
        &self,
        handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
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
