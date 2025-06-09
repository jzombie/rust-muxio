use crate::socket_transport::muxio_emit_socket_frame_bytes;
use futures::SinkExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded as unbounded_channel};
use futures::channel::oneshot;
use futures::stream::StreamExt;
use js_sys::Uint8Array;
use muxio::rpc::{RpcDispatcher, RpcRequest, rpc_internals::RpcStreamEvent};
use std::sync::{Arc, Mutex};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::console;

thread_local! {
    static MUXIO_CLIENT_TX_REF: std::cell::RefCell<Option<UnboundedSender<Vec<u8>>>> =
        std::cell::RefCell::new(None);
}

// TODO: Refactor accordingly
#[wasm_bindgen]
pub fn muxio_receive_socket_frame_uint8(inbound_data: Uint8Array) -> Result<(), JsValue> {
    let inbound_bytes = inbound_data.to_vec();

    // Forward to client channel
    MUXIO_CLIENT_TX_REF.with(|cell| {
        if let Some(tx) = cell.borrow().as_ref() {
            let _ = tx.unbounded_send(inbound_bytes);
        } else {
            console::error_1(&"Client TX not initialized".into());
        }
    });

    Ok(())
}

// TODO: Refactor accordingly
fn start_recv_loop(mut rx: UnboundedReceiver<Vec<u8>>) {
    spawn_local(async move {
        while let Some(bytes) = rx.next().await {
            muxio_emit_socket_frame_bytes(bytes.as_slice());

            // TODO: Send to Rust WASM

            // let s = format!("Received: {:?}", msg);
            // console::log_1(&s.into());
        }
    });
}

pub struct RpcWasmClient {
    // TODO: There's probably no reason to use Arc or Mutex here since the client is single-threaded
    pub dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    pub tx: UnboundedSender<Vec<u8>>,
}

impl RpcWasmClient {
    pub async fn new() -> RpcWasmClient {
        let dispatcher = Arc::new(Mutex::new(RpcDispatcher::new()));

        let (tx, rx) = unbounded_channel::<Vec<u8>>();

        start_recv_loop(rx);

        // TODO: Handle accordingly
        // Store tx in TLS
        MUXIO_CLIENT_TX_REF.with(|cell| {
            *cell.borrow_mut() = Some(tx.clone());
        });

        Self { dispatcher, tx }
    }

    // TODO: Use common trait signature
    pub async fn call_rpc<T: Send + 'static, F: Fn(Vec<u8>) -> T + Send + Sync + 'static>(
        dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
        mut tx: UnboundedSender<Vec<u8>>,
        method_id: u64,
        payload: Vec<u8>,
        response_handler: F,
        is_finalized: bool,
    ) -> (Arc<Mutex<RpcDispatcher<'static>>>, T) {
        let (done_tx, done_rx) = oneshot::channel::<T>();
        let done_tx = Arc::new(Mutex::new(Some(done_tx)));
        let done_tx_clone = done_tx.clone();

        dispatcher
            .lock()
            .unwrap() // TODO: Use result type
            .call(
                RpcRequest {
                    method_id,
                    param_bytes: Some(payload),
                    pre_buffered_payload_bytes: None,
                    is_finalized,
                },
                1024, // TODO: Don't hardcode
                move |chunk| {
                    // let _ = tx.send(WsMessage::Binary(Bytes::copy_from_slice(chunk)));
                    let _ = tx.send(chunk.to_vec());
                },
                Some(move |evt| {
                    if let RpcStreamEvent::PayloadChunk { bytes, .. } = evt {
                        let result = response_handler(bytes);
                        let done_tx_clone2 = done_tx_clone.clone();

                        if let Some(tx) = done_tx_clone2.lock().unwrap().take() {
                            let _ = tx.send(result);
                        }
                        // let done_tx_clone2 = done_tx_clone.clone();
                        // tokio::spawn(async move {
                        //     let mut tx_lock = done_tx_clone2.lock().await;
                        //     if let Some(tx) = tx_lock.take() {
                        //         let _ = tx.send(result);
                        //     }
                        // });
                    }
                }),
                true,
            )
            .unwrap();

        let result = done_rx.await.unwrap();
        (dispatcher, result)
    }
}
