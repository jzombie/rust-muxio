use axum::{
    Router,
    extract::ConnectInfo,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use bytes::Bytes;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use muxio::rpc::RpcDispatcher;
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc::unbounded_channel},
};

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

    /// Returns a clone of the underlying RPC service endpoint.
    /// This allows for registering handlers without tying the registration
    /// logic to the server implementation.
    pub fn endpoint(&self) -> Arc<RpcServiceEndpoint<WsSenderContext>> {
        self.endpoint.clone()
    }
    // --------------------------------------------------

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
        ws.on_upgrade(move |socket| server.handle_socket(socket))
    }

    async fn handle_socket(self: Arc<Self>, socket: WebSocket) {
        let mut dispatcher = RpcDispatcher::new();
        let (sender, mut receiver) = socket.split();
        let context = Arc::new(Mutex::new(sender));
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

        while let Some(Ok(Message::Binary(bytes))) = receiver.next().await {
            let tx_clone = tx.clone();
            let on_emit = |chunk: &[u8]| {
                let _ = tx_clone.send(Message::Binary(Bytes::copy_from_slice(chunk)));
            };

            if let Err(err) = self
                .endpoint
                .read_bytes(&mut dispatcher, context.clone(), &bytes, on_emit)
                .await
            {
                tracing::error!("Caught err: {:?}", err);
            }
        }
        tracing::info!("Client disconnected.");
    }
}
