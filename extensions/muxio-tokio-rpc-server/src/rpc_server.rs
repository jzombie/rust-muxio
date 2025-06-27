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
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
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

/// A type alias for the WebSocket sender part, wrapped for shared access.
/// This allows multiple tasks to send messages to a single client.
type WsSenderContext = Arc<Mutex<SplitSink<WebSocket, Message>>>;

/// An RPC server that listens for WebSocket connections and handles RPC calls.
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

    /// Binds to an address and starts the RPC server.
    ///
    /// The address can be any type that implements `ToSocketAddrs`, such as
    /// a string "127.0.0.1:8080" or a `SocketAddr`.
    pub async fn serve<A: ToSocketAddrs>(self, addr: A) -> Result<SocketAddr, axum::BoxError> {
        let listener = TcpListener::bind(addr).await?;
        let server = Arc::new(self);
        server.serve_with_listener(listener).await
    }

    /// Starts the RPC server on a specific host and port.
    ///
    /// This is a convenience wrapper around `serve`. The host can be an IP
    /// address or a hostname.
    pub async fn serve_on(self, host: &str, port: u16) -> Result<SocketAddr, axum::BoxError> {
        // `ToSocketAddrs` can handle "host:port" strings directly, including hostnames.
        let addr = format!("{host}:{port}");
        // Delegate to the existing generic `serve` function.
        self.serve(addr).await
    }

    /// Starts the RPC server with a pre-bound `TcpListener`.
    ///
    /// This is useful for cases like binding to an ephemeral port (port 0) and
    /// then retrieving the actual address.
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
    ///
    /// This method is the entry point for a new client. It splits the WebSocket
    /// into a sender and receiver and spawns the dedicated tasks responsible
    /// for message handling and transport management.
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

    /// Task responsible for sending outbound messages to the client.
    ///
    /// It listens on an MPSC channel for `Message`s (which can be RPC
    /// responses or Pings) and sends them over the WebSocket connection.
    async fn sender_task(context: WsSenderContext, mut rx: mpsc::UnboundedReceiver<Message>) {
        while let Some(msg) = rx.recv().await {
            if context.lock().await.send(msg).await.is_err() {
                break; // Exit if the client has disconnected.
            }
        }
    }

    /// Task responsible for handling all inbound communication from a client.
    ///
    /// This is the core task for a client connection. It performs several duties:
    /// - Periodically sends Ping messages to check for client liveness.
    /// - Listens for incoming messages (Binary, Pong, Close).
    /// - Enforces a timeout, disconnecting clients that don't respond.
    /// - Dispatches incoming binary messages (RPC calls) to the `RpcServiceEndpoint`.
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
