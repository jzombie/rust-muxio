use super::MUXIO_CLIENT_DISPATCHER_REF;
use super::muxio_emit_socket_frame_bytes;
use futures::channel::oneshot;
use muxio::rpc::{RpcRequest, RpcResponse, RpcResultStatus, rpc_internals::RpcStreamEvent};
use std::sync::{Arc, Mutex};
use web_sys::console;

// TODO: Refactor
// TODO: Enable transport to be defined as a parameter
pub async fn call_muxio(
    method_id: u64,
    param_bytes: Option<Vec<u8>>,
    payload: Option<Vec<u8>>,
    max_chunk_size: usize,
) -> Result<Vec<u8>, String> {
    let (tx, rx) = oneshot::channel();
    let tx_cell = Arc::new(Mutex::new(Some(tx)));
    let result_status = Arc::new(Mutex::new(Some(0u8)));
    let result_bytes = Arc::new(Mutex::new(None::<Vec<u8>>));

    let tx_cell_cloned = Arc::clone(&tx_cell);
    let result_status_cloned = Arc::clone(&result_status);
    let result_bytes_cloned = Arc::clone(&result_bytes);

    MUXIO_CLIENT_DISPATCHER_REF.with(|cell| {
        if let Some(dispatcher) = cell.borrow_mut().as_mut() {
            let _ = dispatcher.call(
                RpcRequest {
                    method_id,
                    param_bytes,
                    pre_buffered_payload_bytes: payload,
                    is_finalized: true,
                },
                max_chunk_size,
                muxio_emit_socket_frame_bytes,
                Some(move |evt| match evt {
                    RpcStreamEvent::Header { rpc_header, .. } => {
                        let rpc_response = RpcResponse::from_rpc_header(&rpc_header);
                        let status = rpc_response
                            .result_status
                            .unwrap_or(RpcResultStatus::SystemError.value());
                        *result_status_cloned.lock().unwrap() = Some(status);
                    }
                    RpcStreamEvent::PayloadChunk { bytes, .. } => {
                        *result_bytes_cloned.lock().unwrap() = Some(bytes);
                    }
                    RpcStreamEvent::End { .. } => {
                        if let Some(sender) = tx_cell_cloned.lock().unwrap().take() {
                            let _ = sender.send(());
                        }
                    }
                    _ => {
                        console::error_1(&format!("Unhandled transport event: {:?}", evt).into());
                    }
                }),
                true,
            );
        } else {
            // fallback path if dispatcher not initialized
            if let Some(sender) = tx_cell.lock().unwrap().take() {
                let _ = sender.send(());
            }
            *result_status.lock().unwrap() = Some(RpcResultStatus::SystemError.value());
        }
    });

    // wait for the response signal
    rx.await.map_err(|_| "dropped".to_string())?;

    // check status and return
    match result_status
        .lock()
        .unwrap()
        .unwrap_or(RpcResultStatus::SystemError.value())
    {
        0 => {
            if let Some(bytes) = result_bytes.lock().unwrap().take() {
                Ok(bytes)
            } else {
                Err("No payload received".to_string())
            }
        }
        code => Err(format!("RPC error status: {}", code)),
    }
}

// TODO: Uncomment if ever wanting to call Muxio directly from JS
// #[wasm_bindgen]
// pub fn call_muxio_js(method_id: u64, param_bytes: &[u8], payload_bytes: &[u8]) -> Promise {
//     let param_vec = match param_bytes.len() {
//         0 => None,
//         _ => Some(param_bytes.to_vec()),
//     };

//     let payload_vec = match payload_bytes.len() {
//         0 => None,
//         _ => Some(payload_bytes.to_vec()),

//     };

//     future_to_promise(async move {
//         match call_muxio_inner(method_id, param_vec, payload_vec).await {
//             Ok(bytes) => Ok(js_sys::Uint8Array::from(bytes.as_slice()).into()),
//             Err(msg) => Err(JsValue::from_str(&msg)),
//         }
//     })
// }
