use futures_util::{SinkExt, StreamExt};
use muxio::{frame::FrameDecodeError, rpc::RpcDispatcher};
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
use std::fmt;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{
    Arc,
    // FIX: Remove std::sync::Mutex
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{Mutex, mpsc}; // FIX: Import tokio's Mutex
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

// FIX: Use tokio::sync::Mutex for the state change handler
type RpcTransportStateChangeHandler =
    Arc<Mutex<Option<Box<dyn Fn(RpcTransportState) + Send + Sync>>>>;

pub struct RpcClient {
    dispatcher: Arc<tokio::sync::Mutex<RpcDispatcher<'static>>>,
    /// The endpoint for handling incoming RPC calls from the server.
    endpoint: Arc<RpcServiceEndpoint<()>>,
    tx: mpsc::UnboundedSender<WsMessage>,
    state_change_handler: RpcTransportStateChangeHandler,
    is_connected: Arc<AtomicBool>,
    _task_handles: Vec<JoinHandle<()>>,
}

impl fmt::Debug for RpcClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RpcClient")
            .field("dispatcher", &"Arc<Mutex<RpcDispatcher>>")
            .field("endpoint", &"Arc<RpcServiceEndpoint<()>>")
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
            // FIX: Use blocking_lock in synchronous Drop impl
            if let Ok(guard) = self.state_change_handler.try_lock() {
                if let Some(handler) = guard.as_ref() {
                    handler(RpcTransportState::Disconnected);
                }
            }
        }
    }
}

impl RpcClient {
    pub async fn new(host: &str, port: u16) -> Result<RpcClient, io::Error> {
        let websocket_url = match host.parse::<IpAddr>() {
            Ok(ip) => {
                let socket_addr = SocketAddr::new(ip, port);
                format!("ws://{socket_addr}/ws")
            }
            Err(_) => format!("ws://{host}:{port}/ws"),
        };

        let (ws_stream, _) = connect_async(websocket_url.to_string())
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        let (app_tx, mut app_rx) = mpsc::unbounded_channel::<WsMessage>();
        let (ws_recv_tx, mut ws_recv_rx) =
            mpsc::unbounded_channel::<Option<Result<WsMessage, WsError>>>();

        // FIX: Initialize with tokio::sync::Mutex
        let state_change_handler: RpcTransportStateChangeHandler = Arc::new(Mutex::new(None));
        let is_connected = Arc::new(AtomicBool::new(true));
        let dispatcher = Arc::new(tokio::sync::Mutex::new(RpcDispatcher::new()));
        let endpoint = Arc::new(RpcServiceEndpoint::new());

        let mut task_handles = Vec::new();
        let is_connected_recv = is_connected.clone();
        let state_handler_recv = state_change_handler.clone();
        let endpoint_handle = endpoint.clone();
        // FIX: Clone the dispatcher Arc for each task that needs it.
        let dispatcher_for_recv = dispatcher.clone();
        let dispatcher_for_dispatch = dispatcher.clone();
        let tx_for_handler = app_tx.clone();

        let recv_handle = tokio::spawn(async move {
            while let Some(msg) = ws_receiver.next().await {
                if ws_recv_tx.send(Some(msg)).is_err() {
                    break;
                }
            }
            if is_connected_recv.swap(false, Ordering::SeqCst) {
                // FIX: Use .await on the tokio MutexGuard
                if let Some(handler) = state_handler_recv.lock().await.as_ref() {
                    handler(RpcTransportState::Disconnected);
                }

                // FIX: Use the cloned dispatcher handle for this task
                let mut dispatcher = dispatcher_for_recv.lock().await;
                let error = FrameDecodeError::ReadAfterCancel;
                dispatcher.fail_all_pending_requests(error);
                tracing::warn!("Connection dropped. Failed all pending RPC requests.");
            }
            let _ = ws_recv_tx.send(None);
        });
        task_handles.push(recv_handle);

        let send_handle = tokio::spawn(async move {
            while let Some(msg) = app_rx.recv().await {
                if ws_sender.send(msg).await.is_err() {
                    break;
                }
            }
        });
        task_handles.push(send_handle);

        let dispatch_handle = tokio::spawn(async move {
            while let Some(Some(msg_result)) = ws_recv_rx.recv().await {
                match msg_result {
                    Ok(WsMessage::Binary(bytes)) => {
                        // FIX: Use the cloned dispatcher handle for this task
                        let mut dispatcher = dispatcher_for_dispatch.lock().await;
                        let on_emit = |chunk: &[u8]| {
                            let _ = tx_for_handler.send(WsMessage::Binary(chunk.to_vec().into()));
                        };
                        let _ = endpoint_handle
                            .read_bytes(&mut dispatcher, (), &bytes, on_emit)
                            .await;
                    }
                    Ok(WsMessage::Ping(data)) => {
                        let _ = tx_for_handler.send(WsMessage::Pong(data));
                    }
                    Ok(WsMessage::Close(_)) => break,
                    Err(e) => {
                        tracing::error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });
        task_handles.push(dispatch_handle);

        Ok(RpcClient {
            dispatcher,
            endpoint,
            tx: app_tx,
            state_change_handler,
            is_connected,
            _task_handles: task_handles,
        })
    }

    pub fn get_endpoint(&self) -> Arc<RpcServiceEndpoint<()>> {
        self.endpoint.clone()
    }
}

#[async_trait::async_trait(?Send)]
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

    async fn set_state_change_handler(
        &self,
        handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        let mut state_handler = self.state_change_handler.lock().await; // Use .await instead of .blocking_lock()
        *state_handler = Some(Box::new(handler));

        if self.is_connected.load(Ordering::SeqCst) {
            if let Some(h) = state_handler.as_ref() {
                h(RpcTransportState::Connected);
            }
        }
    }
}
