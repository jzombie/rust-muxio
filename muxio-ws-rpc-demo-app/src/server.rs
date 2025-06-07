use crate::service_definition::{AddRequestParams, AddResponseParams};
use axum::{
    Router,
    extract::ConnectInfo,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use bitcode::{Decode, Encode};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use muxio::rpc::{
    RpcDispatcher, RpcRequest, RpcResponse, RpcResultStatus, rpc_internals::RpcStreamEvent,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::{
    net::TcpListener,
    sync::{
        Mutex,
        mpsc::{self, unbounded_channel},
        oneshot,
    },
    time::{Duration, sleep},
};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

pub async fn run_server(address: &str) {
    let app = Router::new().route("/ws", get(ws_handler));
    let listener = TcpListener::bind(address).await.unwrap();
    println!("Server running on {:?}", address);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    println!("Client connected: {}", addr);
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(socket: WebSocket) {
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

            let response = match request.method_id {
                0x01 => {
                    let req: AddRequestParams = bitcode::decode(payload).unwrap();
                    let sum = req.numbers.iter().sum();
                    let encoded = bitcode::encode(&AddResponseParams { result: sum });

                    RpcResponse {
                        request_header_id: request_id,
                        method_id: 0x01,
                        result_status: Some(RpcResultStatus::Success.value()),
                        pre_buffered_payload_bytes: Some(encoded),
                        is_finalized: true,
                    }
                }
                _ => RpcResponse {
                    request_header_id: request_id,
                    method_id: request.method_id,
                    result_status: Some(RpcResultStatus::SystemError.value()),
                    pre_buffered_payload_bytes: None,
                    is_finalized: true,
                },
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
