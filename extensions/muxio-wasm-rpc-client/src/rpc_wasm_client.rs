use crate::socket_transport::muxio_emit_socket_frame_bytes;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded as unbounded_channel};
use futures::channel::oneshot;
use futures::stream::StreamExt;
use js_sys::Uint8Array;
use muxio::rpc::{
    RpcDispatcher, RpcRequest,
    rpc_internals::{RpcStreamEncoder, RpcStreamEvent},
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::console;

thread_local! {
    static MUXIO_CLIENT_TX_REF: RefCell<Option<UnboundedSender<Vec<u8>>>> =
        RefCell::new(None);
    static MUXIO_CLIENT_RX_REF: RefCell<Option<Rc<RefCell<UnboundedReceiver<Vec<u8>>>>>> =
        RefCell::new(None);
}

#[wasm_bindgen]
pub fn muxio_receive_socket_frame_uint8(inbound_data: Uint8Array) -> Result<(), JsValue> {
    let inbound_bytes = inbound_data.to_vec();

    MUXIO_CLIENT_TX_REF.with(|cell| {
        if let Some(tx) = cell.borrow().as_ref() {
            let _ = tx.unbounded_send(inbound_bytes);
            web_sys::console::log_1(&"emit...".into());
        } else {
            console::error_1(&"Client TX not initialized".into());
        }
    });

    Ok(())
}

fn start_recv_loop(rx: Rc<RefCell<UnboundedReceiver<Vec<u8>>>>) {
    spawn_local(async move {
        loop {
            let maybe_bytes = rx.borrow_mut().next().await;
            if let Some(bytes) = maybe_bytes {
                web_sys::console::log_1(&"receive...".into());
                muxio_emit_socket_frame_bytes(bytes.as_slice());
            } else {
                break;
            }
        }
    });
}

pub struct RpcWasmClient {
    pub dispatcher: Arc<std::sync::Mutex<RpcDispatcher<'static>>>,
    pub tx: UnboundedSender<Vec<u8>>,
}

impl RpcWasmClient {
    pub async fn new() -> RpcWasmClient {
        let dispatcher = Arc::new(std::sync::Mutex::new(RpcDispatcher::new()));
        let (tx, rx) = unbounded_channel::<Vec<u8>>();
        let rx_rc = Rc::new(RefCell::new(rx));

        MUXIO_CLIENT_TX_REF.with(|cell| {
            *cell.borrow_mut() = Some(tx.clone());
        });

        MUXIO_CLIENT_RX_REF.with(|cell| {
            *cell.borrow_mut() = Some(rx_rc.clone());
        });

        start_recv_loop(rx_rc);

        Self { dispatcher, tx }
    }

    pub async fn call_rpc<T, F>(
        &self,
        method_id: u64,
        payload: Vec<u8>,
        response_handler: F,
        is_finalized: bool,
    ) -> (
        RpcStreamEncoder<Box<dyn for<'a> FnMut(&'a [u8]) + Send + 'static>>,
        T,
    )
    where
        T: Send + 'static,
        F: Fn(Vec<u8>) -> T + Send + Sync + 'static,
    {
        web_sys::console::log_1(&"Call RPC...".into());

        let tx = self.tx.clone();
        let (done_tx, done_rx) = oneshot::channel::<T>();
        let done_tx = Arc::new(std::sync::Mutex::new(Some(done_tx)));
        let done_tx_clone = done_tx.clone();

        let send_fn: Box<dyn for<'a> FnMut(&'a [u8]) + Send + 'static> = Box::new(move |chunk| {
            let _ = tx.unbounded_send(chunk.to_vec());
            web_sys::console::log_1(&"emit...".into());
        });

        let recv_fn: Box<dyn FnMut(RpcStreamEvent) + Send + 'static> = Box::new(move |evt| {
            if let RpcStreamEvent::PayloadChunk { bytes, .. } = evt {
                let result = response_handler(bytes);
                let done_tx_clone2 = done_tx_clone.clone();

                spawn_local(async move {
                    if let Some(tx) = done_tx_clone2.lock().unwrap().take() {
                        let _ = tx.send(result);
                    }
                });
            }
        });

        let rpc_stream_encoder = self
            .dispatcher
            .lock()
            .unwrap()
            .call(
                RpcRequest {
                    method_id,
                    param_bytes: Some(payload),
                    pre_buffered_payload_bytes: None,
                    is_finalized,
                },
                1024,
                send_fn,
                Some(recv_fn),
                true,
            )
            .expect("dispatcher.call failed");

        let result = done_rx.await.expect("oneshot receive failed");

        (rpc_stream_encoder, result)
    }
}
