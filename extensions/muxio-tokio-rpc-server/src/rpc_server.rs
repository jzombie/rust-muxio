use axum::{
    Router,
    extract::ConnectInfo,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use muxio::rpc::RpcDispatcher;
use muxio_rpc_service_endpoint::{
    RpcPrebufferedHandler, RpcServiceEndpoint, RpcServiceEndpointInterface,
};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc::unbounded_channel},
};

/// A type alias for the shareable WebSocket sender, used as the RPC context.
type WsSenderContext = Arc<Mutex<SplitSink<WebSocket, Message>>>;

// TODO: Document that this is a basic server implementation and that the underlying service
// endpoints can be used with alternative servers or transports.

/// An RpcServer specialized for WebSocket connections.
/// It uses the WebSocket's sender as the context for RPC handlers.
pub struct RpcServer {
    endpoint: Arc<RpcServiceEndpoint<WsSenderContext>>,
}

impl Default for RpcServer {
    fn default() -> Self {
        Self::new()
    }
}

impl RpcServer {
    /// Creates a new RpcServer instance with no routes started.
    pub fn new() -> Self {
        RpcServer {
            endpoint: Arc::new(RpcServiceEndpoint::new()),
        }
    }

    /// Starts serving the RPC server with already registered handlers
    /// on the given address. This consumes the server instance.
    pub async fn serve(self, address: &str) -> Result<SocketAddr, axum::BoxError> {
        let listener = TcpListener::bind(address).await?;
        // Wrap the server in an Arc to allow for shared, thread-safe access.
        let server = Arc::new(self);
        server.serve_with_listener(listener).await
    }

    /// Starts serving the RPC server using a pre-bound TcpListener.
    pub async fn serve_with_listener(
        self: Arc<Self>,
        listener: TcpListener,
    ) -> Result<SocketAddr, axum::BoxError> {
        let addr = listener.local_addr()?;

        let app = Router::new().route(
            "/ws", // TODO: Don't hardcode
            get({
                // Clone the Arc to move it into the handler closure.
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

    /// WebSocket route handler that sets up the WebSocket connection.
    /// It receives an `Arc<RpcServer>` to share the server's state.
    async fn ws_handler(
        ws: WebSocketUpgrade,
        ConnectInfo(addr): ConnectInfo<SocketAddr>,
        server: Arc<RpcServer>,
    ) -> impl IntoResponse {
        tracing::info!("Client connected: {}", addr);
        // The `on_upgrade` closure now captures the `server` Arc
        // and calls the `handle_socket` method on it.
        ws.on_upgrade(move |socket| server.handle_socket(socket))
    }

    /// Handles the actual WebSocket connection lifecycle.
    /// This is now a method on RpcServer, allowing access to `self`.
    async fn handle_socket(self: Arc<Self>, socket: WebSocket) {
        let (sender, mut receiver) = socket.split();

        // Wrap the sender in an Arc<Mutex> to make it a shareable context.
        let context = Arc::new(Mutex::new(sender));

        // The write loop now uses the shared context to send messages.
        let (tx, mut rx) = unbounded_channel::<Message>();
        tokio::spawn({
            let context = context.clone();
            async move {
                while let Some(msg) = rx.recv().await {
                    if context.lock().await.send(msg).await.is_err() {
                        break;
                    }
                }
            }
        });

        // The read loop forwards received messages for processing.
        tokio::spawn(async move {
            let (recv_tx, mut recv_rx) =
                unbounded_channel::<Option<Result<Message, axum::Error>>>();
            tokio::spawn(async move {
                while let Some(msg) = receiver.next().await {
                    let done = msg.is_err();
                    if recv_tx.send(Some(msg)).is_err() {
                        break;
                    }
                    if done {
                        break;
                    }
                }
                let _ = recv_tx.send(None);
            });

            while let Some(Some(Ok(Message::Binary(bytes)))) = recv_rx.recv().await {
                let tx_clone = tx.clone();

                // TODO: Remove
                // println!("Received bytes: {:?}, chunk size: {:?}", bytes, bytes.len());

                // The `on_emit` closure sends RPC responses back via the WebSocket.
                let on_emit = |chunk: &[u8]| {
                    let _ = tx_clone.send(Message::Binary(Bytes::copy_from_slice(chunk)));
                };

                // The context (the shareable WebSocket sender) is passed to `read_bytes`.
                if let Err(err) = self.read_bytes(context.clone(), &bytes, on_emit).await {
                    tracing::error!("Caught err: {:?}", err);
                }
            }
        });
    }
}

// FIX: The trait implementation now specifies the concrete context type.
#[async_trait::async_trait]
impl RpcServiceEndpointInterface<WsSenderContext> for RpcServer {
    type DispatcherLock = Mutex<RpcDispatcher<'static>>;
    type HandlersLock = Mutex<HashMap<u64, RpcPrebufferedHandler<WsSenderContext>>>;

    /// Provides access to the dispatcher by delegating to the inner endpoint.
    fn get_dispatcher(&self) -> Arc<Self::DispatcherLock> {
        self.endpoint.get_dispatcher()
    }

    /// Provides access to the handler map by delegating to the inner endpoint.
    fn get_prebuffered_handlers(&self) -> Arc<Self::HandlersLock> {
        self.endpoint.get_prebuffered_handlers()
    }
}
