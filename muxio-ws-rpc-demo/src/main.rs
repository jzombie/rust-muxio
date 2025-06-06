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
use tokio::{
    net::TcpListener,
    time::{Duration, sleep},
};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

#[derive(Encode, Decode, PartialEq, Debug)]
struct AddRequestParams {
    numbers: Vec<f64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
struct AddResponseParams {
    result: f64,
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    println!("Client connected: {}", addr);
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    let mut dispatcher = RpcDispatcher::new();

    while let Some(Ok(msg)) = socket.next().await {
        if let Message::Binary(bytes) = msg {
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

                let mut chunks = Vec::new();
                dispatcher
                    .respond(response, 1024, |chunk| {
                        chunks.push(Bytes::copy_from_slice(chunk));
                    })
                    .unwrap();

                for chunk in chunks {
                    if socket.send(Message::Binary(chunk)).await.is_err() {
                        break;
                    }
                }
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
    sleep(Duration::from_millis(300)).await;
    let (mut ws_stream, _) = connect_async("ws://127.0.0.1:3000/ws")
        .await
        .expect("Failed to connect");

    let payload = bitcode::encode(&AddRequestParams {
        numbers: vec![1.0, 2.0, 3.0],
    });

    let mut client_dispatcher = RpcDispatcher::new();
    let mut outgoing = Vec::new();

    client_dispatcher
        .call(
            RpcRequest {
                method_id: 0x01,
                param_bytes: Some(payload),
                pre_buffered_payload_bytes: None,
                is_finalized: true,
            },
            1024,
            |chunk| outgoing.extend_from_slice(chunk),
            Some(|evt| match evt {
                RpcStreamEvent::PayloadChunk { bytes, .. } => {
                    let decoded: AddResponseParams = bitcode::decode(&bytes).unwrap();

                    println!("decoded: {:?}", decoded);
                }
                _ => println!("Client received evt: {:?}", evt),
            }),
            true,
        )
        .unwrap();

    ws_stream
        .send(WsMessage::Binary(outgoing.into()))
        .await
        .unwrap();

    while let Some(Ok(WsMessage::Binary(bytes))) = ws_stream.next().await {
        client_dispatcher.receive_bytes(&bytes);
        // let response_ids = client_dispatcher.receive_bytes(&bytes).unwrap_or_default();

        // for request_id in response_ids {
        //     if !client_dispatcher
        //         .is_rpc_request_finalized(request_id)
        //         .unwrap_or(false)
        //     {
        //         continue;
        //     }

        //     let Some(response) = client_dispatcher.delete_rpc_request(request_id) else {
        //         continue;
        //     };

        //     let decoded: AddResponseParams =
        //         bitcode::decode(&response.pre_buffered_payload_bytes.unwrap()).unwrap();
        //     println!("Add result: {}", decoded.result);
        //     return;
        // }
    }
}

#[tokio::main]
async fn main() {
    tokio::spawn(run_server());
    run_client().await;
}
