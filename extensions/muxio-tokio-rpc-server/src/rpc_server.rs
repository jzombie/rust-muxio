use axum::{
    Router,
    extract::ConnectInfo,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use muxio_rpc_service::RpcServerInterface;
use muxio_rpc_service_endpoint::RpcServiceEndpoint;
use std::{future::Future, net::SocketAddr, sync::Arc};
use tokio::{net::TcpListener, sync::mpsc::unbounded_channel};

// TODO: Document that this is a basic server implementation and that the underlying service
// endpoints can be used with alternative servers or transports.

pub struct RpcServer {
    endpoint: Arc<RpcServiceEndpoint>,
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
        let listener = TcpListener::bind(address).await.unwrap();
        // Wrap the server in an Arc to allow for shared, thread-safe access.
        let server = Arc::new(self);
        server.serve_with_listener(listener).await
    }

    /// Starts serving the RPC server using a pre-bound TcpListener.
    /// Useful for dynamic ports or external socket management.
    pub async fn serve_with_listener(
        self: Arc<Self>,
        listener: TcpListener,
    ) -> Result<SocketAddr, axum::BoxError> {
        let addr = listener.local_addr().unwrap();

        let app = Router::new().route(
            "/ws", // TODO: Don't hardcode
            get({
                // Clone the Arc to move it into the handler closure.
                let server = self.clone();
                move |ws, conn| Self::ws_handler(ws, conn, server)
            }),
        );

        println!("Server running on {:?}", addr);

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;

        Ok(addr)
    }

    /// WebSocket route handler that sets up the WebSocket connection.
    /// It receives an Arc<RpcServer> to share the server's state.
    async fn ws_handler(
        ws: WebSocketUpgrade,
        ConnectInfo(addr): ConnectInfo<SocketAddr>,
        server: Arc<RpcServer>,
    ) -> impl IntoResponse {
        println!("Client connected: {}", addr);
        // The `on_upgrade` closure now captures the `server` Arc
        // and calls the `handle_socket` method on it.
        ws.on_upgrade(move |socket| server.handle_socket(socket))
    }

    /// Handles the actual WebSocket connection lifecycle.
    /// This is now a method on RpcServer, allowing access to `self`.
    async fn handle_socket(self: Arc<Self>, socket: WebSocket) {
        let (mut sender, mut receiver) = socket.split();
        let (tx, mut rx) = unbounded_channel::<Message>();
        let (recv_tx, mut recv_rx) = unbounded_channel::<Option<Result<Message, axum::Error>>>();

        tokio::spawn(async move {
            while let Some(msg) = receiver.next().await {
                let done = msg.is_err();
                let _ = recv_tx.send(Some(msg));
                if done {
                    break;
                }
            }
            let _ = recv_tx.send(None);
        });

        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if sender.send(msg).await.is_err() {
                    break;
                }
            }
        });

        while let Some(Some(Ok(Message::Binary(bytes)))) = recv_rx.recv().await {
            let tx_clone = tx.clone();

            if let Err(err) = self
                .endpoint
                .read_bytes(&bytes, |chunk| {
                    let _ = tx_clone.send(Message::Binary(Bytes::copy_from_slice(chunk)));
                })
                .await
            {
                eprintln!("Caught err: {:?}", err);
            }
        }
    }
}

#[async_trait::async_trait]
impl RpcServerInterface for RpcServer {
    async fn register_prebuffered<F, Fut>(
        &self,
        method_id: u64,
        handler: F,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: Fn(Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>
            + Send
            + 'static,
    {
        self.endpoint.register_prebuffered(method_id, handler).await
    }
}
