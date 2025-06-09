// mod client_dispatcher;
// pub use client_dispatcher::*;

mod socket_transport;
pub use socket_transport::*;

mod rpc_wasm_client;
pub use rpc_wasm_client::*;

use muxio::rpc::{
    RpcDispatcher, RpcRequest, RpcResponse, RpcResultStatus, rpc_internals::RpcStreamEvent,
};
use std::sync::{Arc, Mutex};
use web_sys::console;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded as unbounded_channel};

use muxio_service_traits::{RpcClientInterface, RpcRequestPrebuffered, RpcResponsePrebuffered};

use std::io;

// TODO: Implement and move into `RpcClient`
#[async_trait::async_trait]
impl RpcClientInterface for RpcWasmClient {
    type Dispatcher = RpcDispatcher<'static>;
    type Sender = UnboundedSender<Vec<u8>>;
    type Mutex<T: Send> = Mutex<T>;

    fn dispatcher(&self) -> Arc<Self::Mutex<Self::Dispatcher>> {
        self.dispatcher.clone()
    }

    fn sender(&self) -> Self::Sender {
        self.tx.clone()
    }

    /// Delegates the call to the actual `RpcClient::call_rpc` implementation.
    async fn call_rpc<T, F>(
        rpc_client: &RpcWasmClient,
        method_id: u64,
        payload: Vec<u8>,
        response_handler: F,
        is_finalized: bool,
    ) -> Result<T, io::Error>
    where
        T: Send + 'static,
        F: Fn(Vec<u8>) -> T + Send + Sync + 'static,
    {
        let (_dispatcher, result) = rpc_client
            .call_rpc(method_id, payload, response_handler, is_finalized)
            .await;

        Ok(result)
    }
}

// TODO: "Original" implementation
// pub async fn call_prebuffered_rpc(
//     method_id: u64,
//     param_bytes: Option<Vec<u8>>,
//     payload: Option<Vec<u8>>,
//     max_chunk_size: usize,
// ) -> Result<Vec<u8>, String> {
//     let (tx, rx) = oneshot::channel();
//     let tx_cell = Arc::new(Mutex::new(Some(tx)));
//     let result_status = Arc::new(Mutex::new(Some(0u8)));
//     let result_bytes = Arc::new(Mutex::new(None::<Vec<u8>>));

//     let tx_cell_cloned = Arc::clone(&tx_cell);
//     let result_status_cloned = Arc::clone(&result_status);
//     let result_bytes_cloned = Arc::clone(&result_bytes);

//     MUXIO_CLIENT_DISPATCHER_REF.with(|cell| {
//         if let Some(dispatcher) = cell.borrow_mut().as_mut() {
//             let _ = dispatcher.call(
//                 RpcRequest {
//                     method_id,
//                     param_bytes,
//                     pre_buffered_payload_bytes: payload,
//                     is_finalized: true,
//                 },
//                 max_chunk_size,
//                 muxio_emit_socket_frame_bytes,
//                 Some(move |evt| match evt {
//                     RpcStreamEvent::Header { rpc_header, .. } => {
//                         let rpc_response = RpcResponse::from_rpc_header(&rpc_header);
//                         let status = rpc_response
//                             .result_status
//                             .unwrap_or(RpcResultStatus::SystemError.value());
//                         *result_status_cloned.lock().unwrap() = Some(status);
//                     }
//                     RpcStreamEvent::PayloadChunk { bytes, .. } => {
//                         *result_bytes_cloned.lock().unwrap() = Some(bytes);
//                     }
//                     RpcStreamEvent::End { .. } => {
//                         if let Some(sender) = tx_cell_cloned.lock().unwrap().take() {
//                             let _ = sender.send(());
//                         }
//                     }
//                     _ => {
//                         console::error_1(&format!("Unhandled transport event: {:?}", evt).into());
//                     }
//                 }),
//                 true,
//             );
//         } else {
//             // fallback path if dispatcher not initialized
//             if let Some(sender) = tx_cell.lock().unwrap().take() {
//                 let _ = sender.send(());
//             }
//             *result_status.lock().unwrap() = Some(RpcResultStatus::SystemError.value());
//         }
//     });

//     // wait for the response signal
//     rx.await.map_err(|_| "dropped".to_string())?;

//     // check status and return
//     match result_status
//         .lock()
//         .unwrap()
//         .unwrap_or(RpcResultStatus::SystemError.value())
//     {
//         0 => {
//             if let Some(bytes) = result_bytes.lock().unwrap().take() {
//                 Ok(bytes)
//             } else {
//                 Err("No payload received".to_string())
//             }
//         }
//         code => Err(format!("RPC error status: {}", code)),
//     }
// }

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

// TODO: Dedupe
pub async fn call_prebuffered_rpc<T, C>(
    rpc_client: &C,
    input: T::Input,
) -> Result<T::Output, io::Error>
where
    T: RpcRequestPrebuffered + RpcResponsePrebuffered + Send + Sync + 'static,
    T::Output: Send + 'static,
    C: RpcClientInterface + Send + Sync,
    C::Dispatcher: Send,
{
    // TODO: Remove
    web_sys::console::log_1(&"Call prebuffered...".into());

    // let dispatcher = rpc_client.dispatcher();
    // let tx = rpc_client.sender();

    let transport_result = rpc_client
        .call_rpc(
            <T as RpcRequestPrebuffered>::METHOD_ID,
            T::encode_request(input),
            T::decode_response,
            true,
        )
        .await?;

    // Error propagation is handled in two steps using two named variables:
    //
    // 1. `transport_result`: Result<Result<T::Output, io::Error>, io::Error>
    //    - This comes from the transport layer (e.g., socket communication).
    //    - The outer Result represents transport-level errors (e.g., channel closed).
    //
    // 2. `rpc_result`: T::Output
    //    - This unwraps the inner Result from `transport_result`.
    //    - If the remote RPC logic failed, this propagates that application-level error.
    let rpc_result = transport_result?;
    Ok(rpc_result)
}
