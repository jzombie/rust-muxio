use axum::{
    Router,
    extract::ConnectInfo,
    extract::State,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::Request,
    response::IntoResponse,
    routing::get,
};
use bitcode::{Decode, Encode};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tokio::time::{Duration, sleep};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

#[derive(Encode, Decode, PartialEq, Debug)]
struct AddRequestParams {
    numbers: Vec<f64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
struct AddResponseParams {
    result: f64,
}

#[derive(Encode, Decode, Debug)]
struct RpcRequest {
    method_id: u64,
    param_bytes: Option<Vec<u8>>,
    pre_buffered_payload_bytes: Option<Vec<u8>>,
    is_finalized: bool,
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    println!("Client connected: {}", addr);
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    while let Some(Ok(msg)) = socket.next().await {
        if let Message::Binary(bytes) = msg {
            let rpc: RpcRequest = bitcode::decode(&bytes).unwrap();
            let method_id = rpc.method_id;

            if method_id == 0x01 {
                let req: AddRequestParams = bitcode::decode(&rpc.param_bytes.unwrap()).unwrap();
                let sum = req.numbers.iter().sum::<f64>();
                let response = AddResponseParams { result: sum };
                let encoded = bitcode::encode(&response);
                let _ = socket.send(Message::Binary(Bytes::from(encoded))).await;
            }
        }
    }
}

async fn run_server() {
    let app = Router::new().route("/ws", get(ws_handler));
    let listener = TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("Server running on 127.0.0.1:3000");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

async fn run_client() {
    sleep(Duration::from_millis(300)).await; // give the server time to bind
    let (mut ws_stream, _) = connect_async("ws://127.0.0.1:3000/ws")
        .await
        .expect("Failed to connect");

    println!("Connected to WebSocket.");

    let payload = bitcode::encode(&AddRequestParams {
        numbers: vec![1.0, 2.0, 3.0],
    });

    let rpc = RpcRequest {
        method_id: 0x01,
        param_bytes: Some(payload),
        pre_buffered_payload_bytes: None,
        is_finalized: true,
    };

    let encoded_rpc = bitcode::encode(&rpc);
    ws_stream
        .send(WsMessage::Binary(Bytes::from(encoded_rpc)))
        .await
        .expect("Failed to send");

    println!("Request sent.");

    while let Some(Ok(WsMessage::Binary(bytes))) = ws_stream.next().await {
        let decoded: AddResponseParams = bitcode::decode(&bytes).unwrap();
        println!("Add result: {:?}", decoded);
        break;
    }
}

#[tokio::main]
async fn main() {
    tokio::spawn(run_server());
    run_client().await;
}
