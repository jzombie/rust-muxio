use axum::{
    Router,
    extract::ConnectInfo,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use bytes::Bytes;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use muxio::rpc::RpcDispatcher;
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc},
    time::timeout,
};

const HEARTBEAT_INTERVAL: u64 = 5;
const CLIENT_TIMEOUT: u64 = 15;

type WsSenderContext = Arc<Mutex<SplitSink<WebSocket, Message>>>;

pub struct RpcServer {
    endpoint: Arc<RpcServiceEndpoint<WsSenderContext>>,
}

impl Default for RpcServer {
    fn default() -> Self {
        Self::new()
    }
}

impl RpcServer {
    pub fn new() -> Self {
        RpcServer {
            endpoint: Arc::new(RpcServiceEndpoint::new()),
        }
    }

    /// Returns an `Arc` clone of the underlying RPC service endpoint.
    /// This allows for registering handlers without tying the registration
    /// logic to the server implementation.
    pub fn endpoint(&self) -> Arc<RpcServiceEndpoint<WsSenderContext>> {
        self.endpoint.clone()
    }

    pub async fn serve(self, address: &str) -> Result<SocketAddr, axum::BoxError> {
        let listener = TcpListener::bind(address).await?;
        let server = Arc::new(self);
        server.serve_with_listener(listener).await
    }

    pub async fn serve_with_listener(
        self: Arc<Self>,
        listener: TcpListener,
    ) -> Result<SocketAddr, axum::BoxError> {
        let addr = listener.local_addr()?;
        let app = Router::new().route(
            "/ws",
            get({
                let server = self.clone();
                move |ws, conn| Self::ws_handler(ws, conn, server)
            }),
        );
        tracing::info!("Server running on {:?}", addr);
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;
        Ok(addr)
    }

    async fn ws_handler(
        ws: WebSocketUpgrade,
        ConnectInfo(addr): ConnectInfo<SocketAddr>,
        server: Arc<RpcServer>,
    ) -> impl IntoResponse {
        tracing::info!("Client connected: {}", addr);
        ws.on_upgrade(move |socket| server.handle_socket(socket, addr))
    }

    async fn handle_socket(self: Arc<Self>, socket: WebSocket, addr: SocketAddr) {
        let (sender, receiver) = socket.split();
        let context = Arc::new(Mutex::new(sender));
        let (tx, rx) = mpsc::unbounded_channel::<Message>();

        // Spawn a task to forward messages from the application to the WebSocket sender.
        tokio::spawn(Self::sender_task(context.clone(), rx));

        // Spawn the main task to handle incoming messages and heartbeats.
        tokio::spawn(Self::receiver_task(
            self.endpoint.clone(),
            context,
            receiver,
            tx,
            addr,
        ));
    }

    /// Task to handle sending messages from the app to the WebSocket client.
    async fn sender_task(context: WsSenderContext, mut rx: mpsc::UnboundedReceiver<Message>) {
        while let Some(msg) = rx.recv().await {
            if context.lock().await.send(msg).await.is_err() {
                break; // Exit if the client has disconnected.
            }
        }
    }

    /// Task to handle incoming messages, heartbeats, and timeouts.
    async fn receiver_task(
        endpoint: Arc<RpcServiceEndpoint<WsSenderContext>>,
        context: WsSenderContext,
        mut receiver: SplitStream<WebSocket>,
        tx: mpsc::UnboundedSender<Message>,
        addr: SocketAddr,
    ) {
        let mut dispatcher = RpcDispatcher::new();
        let heartbeat_interval = Duration::from_secs(HEARTBEAT_INTERVAL);
        let client_timeout = Duration::from_secs(CLIENT_TIMEOUT);

        loop {
            // Use tokio::select! to race the heartbeat timer against message reception.
            tokio::select! {
                // This branch fires every `heartbeat_interval`.
                _ = tokio::time::sleep(heartbeat_interval) => {
                    if tx.send(Message::Ping(vec![].into())).is_err() {
                        // If sending a ping fails, the sender task has likely terminated,
                        // meaning the client is gone.
                        tracing::info!("Client {} disconnected (failed to send ping).", addr);
                        break;
                    }
                }

                // This branch fires when a message is received or the timeout is hit.
                result = timeout(client_timeout, receiver.next()) => {
                    match result {
                        // Timeout occurred: No message received within `client_timeout`.
                        Err(_) => {
                            tracing::warn!("Client {} timed out. Closing connection.", addr);
                            break;
                        },
                        // A message was received from the client.
                        Ok(Some(Ok(msg))) => {
                            match msg {
                                Message::Binary(bytes) => {
                                    let tx_clone = tx.clone();
                                    let on_emit = |chunk: &[u8]| {
                                        let _ = tx_clone.send(Message::Binary(Bytes::copy_from_slice(chunk)));
                                    };
                                    if let Err(err) = endpoint.read_bytes(&mut dispatcher, context.clone(), &bytes, on_emit).await {
                                        tracing::error!("Error processing bytes from {}: {:?}", addr, err);
                                    }
                                }
                                // Client responded to our ping, it's still alive.
                                Message::Pong(_) => {
                                    tracing::trace!("Received pong from {}", addr);
                                }
                                // Client initiated a close.
                                Message::Close(_) => {
                                    tracing::info!("Client {} initiated close.", addr);
                                    break;
                                }
                                _ => {} // Ignore other message types like Text or Ping.
                            }
                        }
                        // The client's stream ended or produced an error.
                        Ok(None) | Ok(Some(Err(_))) => {
                            tracing::info!("Client {} disconnected.", addr);
                            break;
                        }
                    }
                }
            }
        }
        // Loop has exited, the client is considered disconnected.
        tracing::info!("Terminated connection for {}.", addr);
    }
}
