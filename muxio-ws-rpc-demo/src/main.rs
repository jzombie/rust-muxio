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

async fn run_server(address: &str) {
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

async fn add(
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    tx: mpsc::UnboundedSender<WsMessage>,
    numbers: Vec<f64>,
) -> f64 {
    let (done_tx, done_rx) = oneshot::channel();
    let done_tx = Arc::new(Mutex::new(Some(done_tx)));
    let done_tx_clone = done_tx.clone();

    let payload = bitcode::encode(&AddRequestParams { numbers });

    dispatcher
        .lock()
        .await
        .call(
            RpcRequest {
                method_id: 0x01,
                param_bytes: Some(payload),
                pre_buffered_payload_bytes: None,
                is_finalized: true,
            },
            1024,
            move |chunk| {
                let _ = tx.send(WsMessage::Binary(Bytes::copy_from_slice(chunk)));
            },
            Some(move |evt| {
                if let RpcStreamEvent::PayloadChunk { bytes, .. } = evt {
                    let decoded: AddResponseParams = bitcode::decode(&bytes).unwrap();
                    println!("decoded: {:?}", decoded);
                    let done_tx_clone2 = done_tx_clone.clone();
                    tokio::spawn(async move {
                        let mut tx_lock = done_tx_clone2.lock().await;
                        if let Some(tx) = tx_lock.take() {
                            let _ = tx.send(decoded.result);
                        }
                    });
                }
            }),
            true,
        )
        .unwrap();

    done_rx.await.unwrap()
}

async fn run_client(websocket_address: &str) {
    sleep(Duration::from_millis(300)).await;
    let (ws_stream, _) = connect_async(websocket_address)
        .await
        .expect("Failed to connect");
    let (mut sender, mut receiver) = ws_stream.split();

    let (tx, mut rx) = unbounded_channel::<WsMessage>();
    let (recv_tx, mut recv_rx) =
        unbounded_channel::<Option<Result<WsMessage, tokio_tungstenite::tungstenite::Error>>>();

    let dispatcher = Arc::new(Mutex::new(RpcDispatcher::new()));
    let dispatcher_clone = dispatcher.clone();
    let tx_clone = tx.clone();

    // Receive loop
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

    // Send loop
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming
    let dispatcher_handle = dispatcher.clone();
    tokio::spawn(async move {
        while let Some(Some(Ok(WsMessage::Binary(bytes)))) = recv_rx.recv().await {
            dispatcher_handle.lock().await.receive_bytes(&bytes).ok();
        }
    });

    let result = add(dispatcher_clone, tx_clone, vec![1.0, 2.0, 3.0]).await;
    println!("Result from add(): {}", result);
}

#[tokio::main]
async fn main() {
    tokio::spawn(run_server("127.0.0.1:3000"));
    run_client("ws://127.0.0.1:3000/ws").await;
}
