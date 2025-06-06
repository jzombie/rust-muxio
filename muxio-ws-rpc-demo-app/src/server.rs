use axum::{
    Router,
    extract::ConnectInfo,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use muxio::rpc::{RpcDispatcher, RpcResponse, RpcResultStatus};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc::unbounded_channel},
};

type RpcHandler = Box<dyn Fn(Vec<u8>) -> Vec<u8> + Send + Sync + 'static>;

pub struct RpcServer {
    handlers: Arc<Mutex<HashMap<u64, RpcHandler>>>,
}

impl RpcServer {
    /// Creates a new `RpcServer` instance with no routes started.
    pub fn new() -> Self {
        RpcServer {
            handlers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Starts serving the RPC server with already registered handlers
    /// on the given address.
    pub async fn serve(self, address: &str) -> Result<SocketAddr, axum::BoxError> {
        let listener = TcpListener::bind(address).await.unwrap();
        self.serve_with_listener(listener).await
    }

    /// Starts serving the RPC server using a pre-bound TcpListener.
    /// Useful for dynamic ports or external socket management.
    pub async fn serve_with_listener(
        self,
        listener: TcpListener,
    ) -> Result<SocketAddr, axum::BoxError> {
        let addr = listener.local_addr().unwrap();

        let app = Router::new().route(
            "/ws", // TODO: Don't hardcode
            get({
                let handlers = self.handlers.clone();
                move |ws, conn| Self::ws_handler(ws, conn, handlers.clone())
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

    // TODO: Add ability to register streaming handler
    /// Registers a new RPC method handler.
    pub async fn register<F>(&self, method_id: u64, handler: F)
    where
        F: Fn(Vec<u8>) -> Vec<u8> + Send + Sync + 'static,
    {
        self.handlers
            .lock()
            .await
            .insert(method_id, Box::new(handler));
    }

    /// WebSocket route handler that sets up the WebSocket connection.
    async fn ws_handler(
        ws: WebSocketUpgrade,
        ConnectInfo(addr): ConnectInfo<SocketAddr>,
        handlers: Arc<Mutex<HashMap<u64, RpcHandler>>>,
    ) -> impl IntoResponse {
        println!("Client connected: {}", addr);
        ws.on_upgrade(move |socket| Self::handle_socket(socket, handlers))
    }

    /// Handles the actual WebSocket connection lifecycle, dispatching incoming
    /// RPC messages and sending appropriate responses using the muxio dispatcher.
    async fn handle_socket(socket: WebSocket, handlers: Arc<Mutex<HashMap<u64, RpcHandler>>>) {
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

        let mut dispatcher = RpcDispatcher::new();

        while let Some(Some(Ok(Message::Binary(bytes)))) = recv_rx.recv().await {
            let request_ids = match dispatcher.receive_bytes(&bytes) {
                Ok(ids) => ids,
                Err(e) => {
                    eprintln!("Failed to decode incoming bytes: {e:?}");
                    continue;
                }
            };

            for request_id in request_ids {
                if !dispatcher
                    .is_rpc_request_finalized(request_id)
                    .unwrap_or(false)
                {
                    continue;
                }

                let Some(request) = dispatcher.delete_rpc_request(request_id) else {
                    continue;
                };
                let Some(payload) = &request.param_bytes else {
                    continue;
                };

                let response = if let Some(handler) = handlers.lock().await.get(&request.method_id)
                {
                    let encoded = handler(payload.clone());
                    RpcResponse {
                        request_header_id: request_id,
                        method_id: request.method_id,
                        result_status: Some(RpcResultStatus::Success.value()),
                        pre_buffered_payload_bytes: Some(encoded),
                        is_finalized: true,
                    }
                } else {
                    RpcResponse {
                        request_header_id: request_id,
                        method_id: request.method_id,
                        result_status: Some(RpcResultStatus::SystemError.value()),
                        pre_buffered_payload_bytes: None,
                        is_finalized: true,
                    }
                };

                let tx_clone = tx.clone();
                dispatcher
                    .respond(response, 1024, move |chunk| {
                        let _ = tx_clone.send(Message::Binary(Bytes::copy_from_slice(chunk)));
                    })
                    .unwrap();
            }
        }
    }
}
