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
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
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
pub enum RpcServerEvent {
    ClientConnected(ConnectionContextHandle),
    ClientDisconnected(SocketAddr),
}

pub struct ConnectionContext {
    pub sender: WsSenderContext,
    pub addr: SocketAddr,
    // Each connection gets its own dispatcher for making server-to-client calls.
    pub dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
}

/// A wrapper around `Arc<ConnectionContext>` to satisfy Rust's orphan rule.
/// This local newtype allows us to implement the foreign `RpcServiceCallerInterface` trait.
#[derive(Clone)]
pub struct ConnectionContextHandle(pub Arc<ConnectionContext>);

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
        // TODO: Implement custom authentication hook.
        //
        // 1. Define an `AuthHook` trait:
        //    #[async_trait]
        //    pub trait AuthHook: Send + Sync {
        //        async fn authenticate(&self, req: &axum::http::Request<()>) -> Result<(), axum::http::StatusCode>;
        //    }
        //
        // 2. Add `auth_hook: Option<Arc<dyn AuthHook>>` to the `RpcServer` struct.
        //
        // 3. Before upgrading the connection, check the hook:
        //    if let Some(hook) = &server.auth_hook {
        //        // This requires capturing the original request, which can be done
        //        // by modifying the handler signature to include `axum::http::Request`.
        //        if let Err(status_code) = hook.authenticate(&original_request).await {
        //            return (status_code, status_code.to_string()).into_response();
        //        }
        //    }

        tracing::info!("Client connected: {}", addr);
        ws.on_upgrade(move |socket| server.handle_socket(socket, addr))
    }

    async fn handle_socket(self: Arc<Self>, socket: WebSocket, addr: SocketAddr) {
        let (sender, receiver) = socket.split();

        let context = Arc::new(ConnectionContext {
            sender: Arc::new(Mutex::new(sender)),
            addr,
            dispatcher: Arc::new(Mutex::new(RpcDispatcher::new())),
        });

        if let Some(tx) = &self.event_tx {
            let _ = tx.send(RpcServerEvent::ClientConnected(ConnectionContextHandle(
                context.clone(),
            )));
        }

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

    async fn sender_task(
        context: Arc<ConnectionContext>,
        mut rx: mpsc::UnboundedReceiver<Message>,
    ) {
        while let Some(msg) = rx.recv().await {
            if context.sender.lock().await.send(msg).await.is_err() {
                break;
            }
        }
    }

    async fn receiver_task(
        endpoint: Arc<RpcServiceEndpoint<Arc<ConnectionContext>>>,
        context: Arc<ConnectionContext>,
        mut receiver: SplitStream<WebSocket>,
        tx: mpsc::UnboundedSender<Message>,
        addr: SocketAddr,
        event_tx: Option<mpsc::UnboundedSender<RpcServerEvent>>,
    ) {
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
                                    let mut dispatcher = context.dispatcher.lock().await;
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

        if let Some(tx) = event_tx {
            let _ = tx.send(RpcServerEvent::ClientDisconnected(addr));
        }
    }
}

/// Implements the `RpcServiceCallerInterface` to enable server-initiated RPC calls.
///
/// This implementation allows the server to act as a "client" by using a specific
/// connection handle (`ConnectionContextHandle`) to send new, unsolicited RPC requests
/// to that connected client. This is distinct from simply replying to a client's
/// initial request and is the foundation for fully bidirectional communication,
/// such as server-push notifications or commands.
#[async_trait::async_trait]
impl RpcServiceCallerInterface for ConnectionContextHandle {
    // The dispatcher is protected by a Tokio Mutex.
    type DispatcherLock = Mutex<RpcDispatcher<'static>>;

    fn get_dispatcher(&self) -> Arc<Self::DispatcherLock> {
        // Return the dispatcher associated with this specific connection.
        self.0.dispatcher.clone()
    }

    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        // Returns a thread-safe function that sends a raw byte payload
        // over the WebSocket connection associated with this specific client.
        // This is the lowest-level transport function used by the RPC caller.
        Arc::new({
            let sender = self.0.sender.clone();
            move |chunk: Vec<u8>| {
                let sender = sender.clone();
                tokio::spawn(async move {
                    let _ = sender
                        .lock()
                        .await
                        .send(Message::Binary(chunk.into()))
                        .await;
                });
            }
        })
    }

    /// This is a client-side concept, so it's a no-op on the server.
    /// The server manages the connection state directly.
    fn set_state_change_handler(
        &self,
        _handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        // It doesn't make sense for the server to set a state change handler
        // on a connection it owns, so we do nothing.
        tracing::warn!(
            "set_state_change_handler called on server-side connection context; this is a no-op."
        );
    }
}
