use futures_util::{SinkExt, StreamExt};
use muxio::{frame::FrameDecodeError, rpc::RpcDispatcher};
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
use std::fmt;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::runtime::Handle;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

type RpcTransportStateChangeHandler =
    Arc<Mutex<Option<Box<dyn Fn(RpcTransportState) + Send + Sync>>>>;

pub struct RpcClient {
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    /// The endpoint for handling incoming RPC calls from the server.
    endpoint: Arc<RpcServiceEndpoint<()>>,
    tx: mpsc::UnboundedSender<WsMessage>,
    state_change_handler: RpcTransportStateChangeHandler,
    is_connected: Arc<AtomicBool>,
    _task_handles: Mutex<Vec<JoinHandle<()>>>,
}

impl fmt::Debug for RpcClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RpcClient")
            .field("dispatcher", &"Arc<Mutex<RpcDispatcher>>")
            .field("endpoint", &"Arc<RpcServiceEndpoint<()>>")
            .field("tx", &"...")
            .field("state_change_handler", &"...")
            .field("is_connected", &self.is_connected.load(Ordering::Relaxed))
            .finish()
    }
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        // The only job of Drop is to clean up the background tasks.
        // The shutdown logic is handled by the `recv_handle` task.
        for handle in self._task_handles.blocking_lock().iter() {
            handle.abort();
        }
    }
}

impl RpcClient {
    /// Creates a new RPC client and connects to a WebSocket server.
    ///
    /// The `host` can be either an IP address (v4 or v6) or a hostname that
    /// will be resolved via DNS.
    pub async fn new(host: &str, port: u16) -> Result<Arc<Self>, io::Error> {
        let websocket_url = match host.parse::<IpAddr>() {
            Ok(ip) => format!("ws://{}/ws", SocketAddr::new(ip, port)),
            Err(_) => format!("ws://{host}:{port}/ws"),
        };
        let (ws_stream, _) = connect_async(&websocket_url)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        let (app_tx, mut app_rx) = mpsc::unbounded_channel::<WsMessage>();

        let client = Arc::new(Self {
            dispatcher: Arc::new(Mutex::new(RpcDispatcher::new())),
            endpoint: Arc::new(RpcServiceEndpoint::new()),
            tx: app_tx.clone(),
            state_change_handler: Arc::new(Mutex::new(None)),
            is_connected: Arc::new(AtomicBool::new(true)),
            _task_handles: Mutex::new(Vec::new()),
        });

        let mut task_handles = Vec::new();

        // Receive loop
        let client_recv = client.clone();
        let recv_handle = tokio::spawn(async move {
            while let Some(msg) = ws_receiver.next().await {
                if let Ok(WsMessage::Binary(bytes)) = msg {
                    let mut dispatcher = client_recv.dispatcher.lock().await;
                    let on_emit = |chunk: &[u8]| {
                        let _ = client_recv
                            .tx
                            .send(WsMessage::Binary(chunk.to_vec().into()));
                    };
                    let _ = client_recv
                        .endpoint
                        .read_bytes(&mut dispatcher, (), &bytes, on_emit)
                        .await;
                }
            }
            client_recv.shutdown().await;
        });
        task_handles.push(recv_handle);

        // Send loop
        let send_handle = tokio::spawn(async move {
            while let Some(msg) = app_rx.recv().await {
                if ws_sender.send(msg).await.is_err() {
                    break;
                }
            }
        });
        task_handles.push(send_handle);

        *client._task_handles.lock().await = task_handles;

        Ok(client)
    }

    /// A single, authoritative method to handle all shutdown and cleanup logic.
    async fn shutdown(&self) {
        if self.is_connected.swap(false, Ordering::SeqCst) {
            let mut dispatcher = self.dispatcher.lock().await;
            let error = FrameDecodeError::ReadAfterCancel;
            dispatcher.fail_all_pending_requests(error);
            tracing::warn!("Connection dropped. Failed all pending RPC requests.");

            if let Some(handler) = self.state_change_handler.lock().await.as_ref() {
                handler(RpcTransportState::Disconnected);
            }
        }
    }

    pub fn get_endpoint(&self) -> Arc<RpcServiceEndpoint<()>> {
        self.endpoint.clone()
    }
}

#[async_trait::async_trait(?Send)]
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

    /// Sets a callback that will be invoked with the current `RpcTransportState`
    /// whenever the WebSocket connection status changes.
    async fn set_state_change_handler(
        &self,
        handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        let mut state_handler = self.state_change_handler.lock().await;
        *state_handler = Some(Box::new(handler));

        if self.is_connected.load(Ordering::Relaxed) {
            if let Some(h) = state_handler.as_ref() {
                h(RpcTransportState::Connected);
            }
        }
    }
}
