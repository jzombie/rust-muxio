use crate::service_definition::{AddRequestParams, AddResponseParams};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use muxio::rpc::{
    RpcDispatcher, RpcRequest, rpc_internals::RpcStreamEvent,
};
use std::sync::Arc;
use tokio::{
    sync::{
        Mutex,
        mpsc::{self, unbounded_channel},
        oneshot,
    },
    time::{Duration, sleep},
};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

pub async fn call_rpc<T: Send + 'static, F: Fn(Vec<u8>) -> T + Send + Sync + 'static>(
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    tx: mpsc::UnboundedSender<WsMessage>,
    method_id: u64,
    payload: Vec<u8>,
    handler: F,
    is_finalized: bool,
) -> (Arc<Mutex<RpcDispatcher<'static>>>, T) {
    let (done_tx, done_rx) = oneshot::channel::<T>();
    let done_tx = Arc::new(Mutex::new(Some(done_tx)));
    let done_tx_clone = done_tx.clone();

    dispatcher
        .lock()
        .await
        .call(
            RpcRequest {
                method_id,
                param_bytes: Some(payload),
                pre_buffered_payload_bytes: None,
                is_finalized,
            },
            1024,
            move |chunk| {
                let _ = tx.send(WsMessage::Binary(Bytes::copy_from_slice(chunk)));
            },
            Some(move |evt| {
                if let RpcStreamEvent::PayloadChunk { bytes, .. } = evt {
                    let result = handler(bytes);
                    let done_tx_clone2 = done_tx_clone.clone();
                    tokio::spawn(async move {
                        let mut tx_lock = done_tx_clone2.lock().await;
                        if let Some(tx) = tx_lock.take() {
                            let _ = tx.send(result);
                        }
                    });
                }
            }),
            true,
        )
        .unwrap();

    let result = done_rx.await.unwrap();
    (dispatcher, result)
}

pub async fn run_client(websocket_address: &str) {
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

// TODO: Move
async fn add(
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    tx: mpsc::UnboundedSender<WsMessage>,
    numbers: Vec<f64>,
) -> f64 {
    let payload = bitcode::encode(&AddRequestParams { numbers });
    let (_dispatcher, result) = call_rpc(
        dispatcher,
        tx,
        0x01,
        payload,
        |bytes| {
            let decoded: AddResponseParams = bitcode::decode(&bytes).unwrap();
            decoded.result
        },
        true,
    )
    .await;

    result
}
