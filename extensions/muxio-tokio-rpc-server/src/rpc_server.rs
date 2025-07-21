//! Note: This `RpcServer` is a reference implementation and does not include
//! authentication or authorization mechanisms. It is best suited for trusted,
//! internal network communication or as a foundational example. Any struct that
//! utilizes an [`RpcServiceEndpoint`] can function as a "server" for handling
//! RPC requests; this implementation demonstrates one way to do so over WebSockets
//! using the Axum web framework.

use axum::{
    Router,
    extract::ConnectInfo,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use bytes::Bytes;
use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use muxio::rpc::RpcDispatcher;
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::{
    net::{TcpListener, ToSocketAddrs},
    sync::{Mutex, mpsc},
    time::timeout,
};

/// The interval at which the server sends Ping messages to the client.
const HEARTBEAT_INTERVAL: u64 = 5;

/// The maximum time to wait for a message from the client (including Pong)
/// before considering the connection timed out.
const CLIENT_TIMEOUT: u64 = 15;

/// Represents events that occur on the `RpcServer`.
#[derive(Debug)]
pub enum RpcServerEvent {
    ClientConnected(SocketAddr),
    ClientDisconnected(SocketAddr),
}

#[derive(Debug)]
pub struct ConnectionContext {
    pub sender: WsSenderContext,
    pub addr: SocketAddr,
}

/// A type alias for the WebSocket sender part, wrapped for shared access.
type WsSenderContext = Arc<Mutex<SplitSink<WebSocket, Message>>>;

/// An RPC server that listens for WebSocket connections and handles RPC calls.
pub struct RpcServer {
    endpoint: Arc<RpcServiceEndpoint<Arc<ConnectionContext>>>,
    event_tx: Option<mpsc::UnboundedSender<RpcServerEvent>>,
}

impl RpcServer {
    /// Creates a new `RpcServer`.
    ///
    /// The optional `event_tx` channel sender can be used to receive
    /// notifications about server events, such as client connections
    /// and disconnections.
    pub fn new(event_tx: Option<mpsc::UnboundedSender<RpcServerEvent>>) -> Self {
        RpcServer {
            endpoint: Arc::new(RpcServiceEndpoint::new()),
            event_tx,
        }
    }

    /// Returns an `Arc` clone of the underlying RPC service endpoint.
    pub fn endpoint(&self) -> Arc<RpcServiceEndpoint<Arc<ConnectionContext>>> {
        self.endpoint.clone()
    }

    /// Binds to an address and starts the RPC server.
    pub async fn serve<A: ToSocketAddrs>(self, addr: A) -> Result<SocketAddr, axum::BoxError> {
        let listener = TcpListener::bind(addr).await?;
        let server = Arc::new(self);
        server.serve_with_listener(listener).await
    }

    /// Starts the RPC server on a specific host and port.
    pub async fn serve_on(self, host: &str, port: u16) -> Result<SocketAddr, axum::BoxError> {
        let addr = format!("{host}:{port}");
        self.serve(addr).await
    }

    /// Starts the RPC server with a pre-bound `TcpListener`.
    pub async fn serve_with_listener(
        self: Arc<Self>,
        listener: TcpListener,
    ) -> Result<SocketAddr, axum::BoxError> {
        let address = listener.local_addr()?;
        let app = Router::new().route(
            "/ws",
            get({
                let server = self.clone();
                move |ws, conn| Self::ws_handler(ws, conn, server)
            }),
        );
        tracing::info!("Server running on {:?}", address);
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;
        Ok(address)
    }

    /// Manages a new, established WebSocket connection.
    async fn ws_handler(
        ws: WebSocketUpgrade,
        ConnectInfo(addr): ConnectInfo<SocketAddr>,
        server: Arc<RpcServer>,
    ) -> impl IntoResponse {
        tracing::info!("Client connected: {}", addr);
        ws.on_upgrade(move |socket| server.handle_socket(socket, addr))
    }

    async fn handle_socket(self: Arc<Self>, socket: WebSocket, addr: SocketAddr) {
        // Send a connected event if a channel is configured.

        if let Some(tx) = &self.event_tx {
            let _ = tx.send(RpcServerEvent::ClientConnected(addr));
        }

        let (sender, receiver) = socket.split();

        let context = Arc::new(ConnectionContext {
            sender: Arc::new(Mutex::new(sender)),
            addr,
        });

        // This MPSC channel is for sending messages *out* to the WebSocket.
        let (tx, rx) = mpsc::unbounded_channel::<Message>();

        tokio::spawn(Self::sender_task(context.clone(), rx));

        let event_tx_clone = self.event_tx.clone();

        tokio::spawn(Self::receiver_task(
            self.endpoint.clone(),
            context,
            receiver,
            tx,
            addr,
            event_tx_clone,
        ));
    }

    /// Task responsible for sending outbound messages to the client.
    async fn sender_task(
        context: Arc<ConnectionContext>,
        mut rx: mpsc::UnboundedReceiver<Message>,
    ) {
        while let Some(msg) = rx.recv().await {
            // Access the sender inside the context
            if context.sender.lock().await.send(msg).await.is_err() {
                break;
            }
        }
    }

    /// Task responsible for handling all inbound communication from a client.
    async fn receiver_task(
        endpoint: Arc<RpcServiceEndpoint<Arc<ConnectionContext>>>,
        context: Arc<ConnectionContext>,
        mut receiver: SplitStream<WebSocket>,
        tx: mpsc::UnboundedSender<Message>,
        addr: SocketAddr,
        event_tx: Option<mpsc::UnboundedSender<RpcServerEvent>>,
    ) {
        let mut dispatcher = RpcDispatcher::new();
        let heartbeat_interval = Duration::from_secs(HEARTBEAT_INTERVAL);
        let client_timeout = Duration::from_secs(CLIENT_TIMEOUT);

        loop {
            tokio::select! {
                _ = tokio::time::sleep(heartbeat_interval) => {
                    if tx.send(Message::Ping(vec![].into())).is_err() {
                        tracing::info!("Client {} disconnected (failed to send ping).", addr);
                        break;
                    }
                }
                result = timeout(client_timeout, receiver.next()) => {
                    match result {
                        Err(_) => {
                            tracing::warn!("Client {} timed out. Closing connection.", addr);
                            break;
                        },
                        Ok(Some(Ok(msg))) => {
                            match msg {
                                Message::Binary(bytes) => {
                                    let tx_clone = tx.clone();
                                    let on_emit = move |chunk: &[u8]| {
                                        let _ = tx_clone.send(Message::Binary(Bytes::copy_from_slice(chunk)));
                                    };
                                    if let Err(err) = endpoint.read_bytes(&mut dispatcher, context.clone(), &bytes, on_emit).await {
                                        tracing::error!("Error processing bytes from {}: {:?}", addr, err);
                                    }
                                }
                                Message::Pong(_) => {
                                    tracing::trace!("Received pong from {}", addr);
                                }
                                Message::Close(_) => {
                                    tracing::info!("Client {} initiated close.", addr);
                                    break;
                                }
                                _ => {}
                            }
                        }
                        Ok(None) | Ok(Some(Err(_))) => {
                            tracing::info!("Client {} disconnected.", addr);
                            break;
                        }
                    }
                }
            }
        }

        // After the loop breaks, send a disconnected event if a channel is configured.
        tracing::info!("Terminated connection for {}.", addr);
        if let Some(tx) = event_tx {
            let _ = tx.send(RpcServerEvent::ClientDisconnected(addr));
        }
    }
}
