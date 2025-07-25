use futures_util::{SinkExt, StreamExt};
use muxio::{frame::FrameDecodeError, rpc::RpcDispatcher};
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
use std::{
    fmt, io,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc, Mutex as StdMutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::{
    sync::{Mutex as TokioMutex, mpsc},
    task::JoinHandle,
};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use tracing::{self, instrument};

type RpcTransportStateChangeHandler =
    Arc<StdMutex<Option<Box<dyn Fn(RpcTransportState) + Send + Sync>>>>;

pub struct RpcClient {
    dispatcher: Arc<TokioMutex<RpcDispatcher<'static>>>,
    endpoint: Arc<RpcServiceEndpoint<()>>,
    tx: mpsc::UnboundedSender<WsMessage>,
    state_change_handler: RpcTransportStateChangeHandler,
    is_connected: Arc<AtomicBool>,
    task_handles: Vec<JoinHandle<()>>,
}

impl fmt::Debug for RpcClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RpcClient")
            .field("is_connected", &self.is_connected.load(Ordering::Relaxed))
            .finish()
    }
}

impl Drop for RpcClient {
    #[instrument(skip(self))]
    fn drop(&mut self) {
        tracing::debug!("Client is being dropped. Aborting tasks and calling shutdown_sync.");
        for handle in &self.task_handles {
            handle.abort();
        }
        self.shutdown_sync();
        tracing::debug!("Client dropped finished.");
    }
}

impl RpcClient {
    #[instrument(skip(self))]
    fn shutdown_sync(&self) {
        tracing::debug!(
            "Entered. Current `is_connected`: {}",
            self.is_connected.load(Ordering::Relaxed)
        );
        if self.is_connected.swap(false, Ordering::SeqCst) {
            tracing::debug!("`is_connected` was true, proceeding with sync shutdown.");
            if let Ok(guard) = self.state_change_handler.lock() {
                if let Some(handler) = guard.as_ref() {
                    tracing::debug!("Calling Disconnected handler (sync path).");
                    handler(RpcTransportState::Disconnected);
                } else {
                    tracing::debug!("No `state_change_handler` set.");
                }
            } else {
                tracing::debug!("Failed to acquire `state_change_handler` lock.");
            }
        } else {
            tracing::debug!("Already disconnected or shutting down.");
        }
        tracing::debug!("Exited.");
    }

    #[instrument(skip(self))]
    async fn shutdown_async(&self) {
        tracing::debug!(
            "Entered. Current is_connected: {}",
            self.is_connected.load(Ordering::Relaxed)
        );
        if self.is_connected.swap(false, Ordering::SeqCst) {
            tracing::debug!("`is_connected` was true, proceeding with async shutdown.");
            if let Ok(guard) = self.state_change_handler.lock() {
                if let Some(handler) = guard.as_ref() {
                    tracing::debug!(
                        "Calling `RpcTransportState::Disconnected` handler (async path)."
                    );
                    handler(RpcTransportState::Disconnected);
                } else {
                    tracing::debug!("No state_change_handler set.");
                }
            } else {
                tracing::debug!("Failed to acquire state_change_handler lock.");
            }
            // Ensure dispatcher lock is acquired to prevent other RPC calls during shutdown
            let mut dispatcher = self.dispatcher.lock().await;
            tracing::debug!("Acquired dispatcher lock.");
            dispatcher.fail_all_pending_requests(FrameDecodeError::ReadAfterCancel);
            tracing::debug!("All pending requests failed.");
        } else {
            tracing::debug!("Already disconnected or shutting down.");
        }
        tracing::debug!("Exited.");
    }

    #[instrument]
    pub async fn new(host: &str, port: u16) -> Result<Arc<Self>, io::Error> {
        let websocket_url = match host.parse::<IpAddr>() {
            Ok(ip) => format!("ws://{}/ws", SocketAddr::new(ip, port)),
            Err(_) => format!("ws://{host}:{port}/ws"),
        };
        tracing::debug!("Attempting to connect to: {}", websocket_url);

        let (ws_stream, response) = connect_async(&websocket_url).await.map_err(|e| {
            tracing::debug!("Connection failed: {}", e);
            io::Error::new(io::ErrorKind::ConnectionRefused, e)
        })?;
        tracing::debug!(
            "Successfully connected to WebSocket. Response status: {}",
            response.status()
        );

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        let (app_tx, mut app_rx) = mpsc::unbounded_channel::<WsMessage>();
        tracing::debug!("WebSocket stream split and MPSC channel created.");

        let client = Arc::new_cyclic(|weak_client: &Weak<RpcClient>| {
            let state_change_handler: RpcTransportStateChangeHandler =
                Arc::new(StdMutex::new(None));
            let is_connected = Arc::new(AtomicBool::new(true));
            let dispatcher = Arc::new(TokioMutex::new(RpcDispatcher::new()));
            let endpoint = Arc::new(RpcServiceEndpoint::new());
            let mut task_handles = Vec::new();

            // Minimal heartbeat task to generate traffic
            let heartbeat_tx = app_tx.clone();
            let heartbeat_handle = tokio::spawn(async move {
                tracing::debug!("Starting heartbeat task.");
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                loop {
                    interval.tick().await;
                    if heartbeat_tx.send(WsMessage::Ping(vec![].into())).is_err() {
                        tracing::debug!("Failed to send ping, channel likely closed. Exiting.");
                        break;
                    }
                    tracing::debug!("Sent ping.");
                }
                tracing::debug!("Heartbeat task finished.");
            });
            task_handles.push(heartbeat_handle);

            // Receive loop
            let client_weak_recv = weak_client.clone();
            let recv_handle = tokio::spawn(async move {
                tracing::debug!("Starting receive loop.");
                while let Some(msg_result) = ws_receiver.next().await {
                    if let Some(client) = client_weak_recv.upgrade() {
                        match msg_result {
                            Ok(WsMessage::Binary(bytes)) => {
                                tracing::debug!("Received binary message ({} bytes).", bytes.len());
                                let mut dispatcher = client.dispatcher.lock().await;
                                let on_emit = |chunk: &[u8]| {
                                    let _ =
                                        client.tx.send(WsMessage::Binary(chunk.to_vec().into()));
                                    tracing::debug!(
                                        "Emitted binary chunk ({} bytes).",
                                        chunk.len()
                                    );
                                };
                                let _ = client
                                    .endpoint
                                    .read_bytes(&mut dispatcher, (), &bytes, on_emit)
                                    .await;
                            }
                            Ok(WsMessage::Ping(data)) => {
                                tracing::debug!("Received Ping message.");
                                let _ = client.tx.send(WsMessage::Pong(data.into()));
                            }
                            Ok(msg) => {
                                tracing::debug!("Received other WebSocket message: {:?}", msg);
                            }
                            Err(e) => {
                                tracing::debug!("WebSocket receive error: {:?}", e);
                                // An error here often means the connection is broken.
                                if let Some(client) = client_weak_recv.upgrade() {
                                    tracing::error!(
                                        "Upgraded client, spawning shutdown_async due to receive error."
                                    );
                                    tokio::spawn(async move {
                                        client.shutdown_async().await;
                                    });
                                }
                                break; // Exit loop on error
                            }
                        }
                    } else {
                        tracing::warn!("Client Arc dropped while in loop. Exiting.");
                        break;
                    }
                }
                // This block is executed when ws_receiver.next().await returns None (stream ended)
                // or if client_weak_recv.upgrade() fails in a subsequent loop iteration, or break is hit.
                tracing::debug!(
                    "`ws_receiver` stream ended or loop broke. Attempting final `shutdown_async`."
                );
                if let Some(client) = client_weak_recv.upgrade() {
                    tracing::debug!("Client upgraded for final `shutdown_async`.");
                    tokio::spawn(async move {
                        client.shutdown_async().await;
                    });
                } else {
                    tracing::debug!(
                        "Client Arc already dropped at end of loop, cannot call `shutdown_async`."
                    );
                }
                tracing::debug!("Receive loop finished.");
            });
            task_handles.push(recv_handle);

            // Send loop
            let client_weak_send = weak_client.clone();
            let is_connected_send = is_connected.clone(); // Clone is_connected for this task
            let send_handle = tokio::spawn(async move {
                tracing::debug!("Starting send loop.");
                while let Some(msg) = app_rx.recv().await {
                    // Check if client is still considered connected before attempting to send
                    if !is_connected_send.load(Ordering::Acquire) {
                        // Use Acquire for strong ordering
                        tracing::debug!("Client is disconnected. Dropping message: {:?}", msg);
                        // Don't try to send, just break or continue to drain if necessary
                        break; // Exit loop if disconnected
                    }

                    tracing::debug!("Sending message: {:?}", msg);
                    if ws_sender.send(msg).await.is_err() {
                        tracing::debug!(
                            "`ws_sender` failed to send message. Attempting `shutdown_async`."
                        );
                        if let Some(client) = client_weak_send.upgrade() {
                            tokio::spawn(async move {
                                client.shutdown_async().await;
                            });
                        } else {
                            tracing::debug!(
                                "Client Arc already dropped, cannot call `shutdown_async`."
                            );
                        }
                        break; // Break loop on send error
                    }
                }
                tracing::debug!("Send loop finished.");
            });
            task_handles.push(send_handle);

            Self {
                dispatcher,
                endpoint,
                tx: app_tx,
                state_change_handler,
                is_connected,
                task_handles,
            }
        });

        tracing::debug!("Client instance created successfully.");
        Ok(client)
    }

    pub fn get_endpoint(&self) -> Arc<RpcServiceEndpoint<()>> {
        self.endpoint.clone()
    }
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcClient {
    fn get_dispatcher(&self) -> Arc<TokioMutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    #[instrument(skip(self))]
    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        Arc::new({
            let tx = self.tx.clone();
            let is_connected_clone = self.is_connected.clone();
            move |chunk: Vec<u8>| {
                if !is_connected_clone.load(Ordering::Relaxed) {
                    tracing::debug!("Client is disconnected, dropping outgoing RPC data.");
                    return; // Do not send if disconnected
                }

                let chunk_len = chunk.len();
                let send_result = tx.send(WsMessage::Binary(chunk.into()));
                match send_result {
                    Ok(_) => {
                        tracing::debug!("Emitted binary chunk ({} bytes) via mpsc.", chunk_len)
                    }
                    Err(e) => tracing::debug!(
                        "Failed to send binary chunk ({} bytes) via mpsc: {}",
                        chunk_len,
                        e
                    ),
                }
            }
        })
    }

    #[instrument(skip(self, handler))]
    async fn set_state_change_handler(
        &self,
        handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        let mut state_handler = self.state_change_handler.lock().unwrap();
        *state_handler = Some(Box::new(handler));
        tracing::debug!("Handler set.");

        if self.is_connected.load(Ordering::Relaxed) {
            if let Some(h) = state_handler.as_ref() {
                tracing::debug!("Calling Connected handler (initial state).");
                h(RpcTransportState::Connected);
            } else {
                tracing::error!("Handler disappeared after setting?");
            }
        } else {
            tracing::debug!("Client not connected, skipping initial Connected call.");
        }
    }
}
