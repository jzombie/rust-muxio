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
    pub mpsc_tx: mpsc::UnboundedSender<Message>,
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
        let (sender_ws, receiver_ws) = socket.split();

        // Create the internal mpsc channel for this connection
        let (tx_mpsc, rx_mpsc) = mpsc::unbounded_channel::<Message>(); // Renamed for clarity

        let context = Arc::new(ConnectionContext {
            sender: Arc::new(Mutex::new(sender_ws)), // Renamed for clarity from 'sender'
            mpsc_tx: tx_mpsc.clone(),                // <--- USE THE CLONED SENDER HERE
            addr,
            dispatcher: Arc::new(Mutex::new(RpcDispatcher::new())),
        });

        if let Some(tx_event) = &self.event_tx {
            // Renamed for clarity
            let _ = tx_event.send(RpcServerEvent::ClientConnected(ConnectionContextHandle(
                context.clone(),
            )));
        }

        // Pass the receiver to the sender_task
        tokio::spawn(Self::sender_task(context.clone(), rx_mpsc));

        // Pass the sender to the receiver_task (for responding to client-initiated calls)
        let event_tx_clone = self.event_tx.clone();
        tokio::spawn(Self::receiver_task(
            self.endpoint.clone(),
            context,
            receiver_ws, // Renamed for clarity from 'receiver'
            tx_mpsc,     // <--- Pass tx_mpsc here as well
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
                                    println!("BEFORE DISPATCHER LOCK");
                                    let mut dispatcher = context.dispatcher.lock().await;
                                    println!("AFTER DISPATCHER LOCK");
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
    fn get_dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>> {
        // Return the dispatcher associated with this specific connection.
        self.0.dispatcher.clone()
    }

    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        Arc::new({
            let mpsc_tx = self.0.mpsc_tx.clone(); // <--- CLONE THE MPSC SENDER

            // This closure will be called by the RpcDispatcher/RpcServiceCallerInterface.
            // It must be synchronous (not `async move` returning a Future)
            // and should not block.
            move |chunk: Vec<u8>| {
                // This now sends the message to the internal MPSC channel, which is non-blocking.
                // The actual async WebSocket send happens in the separate sender_task.
                let _ = mpsc_tx.send(Message::Binary(chunk.into()));
                // Ignoring the error if the receiver is dropped is acceptable for a "fire and forget"
                // emit_fn, as the sender_task will handle the actual WebSocket error.
            }
        })
    }

    /// This is a client-side concept, so it's a no-op on the server.
    /// The server manages the connection state directly.
    async fn set_state_change_handler(
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
