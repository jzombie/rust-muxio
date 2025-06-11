use super::static_muxio_emit_frame_bytes;
use crate::RpcWasmClient;
use js_sys::Promise;
use std::cell::RefCell;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

thread_local! {
    pub static MUXIO_STATIC_RPC_CLIENT_REF: RefCell<Option<Arc<RpcWasmClient>>> = RefCell::new(None);
}

/// Safe to call multiple times; only the first invocation has effect.
pub fn init_static_client() {
    MUXIO_STATIC_RPC_CLIENT_REF.with(|cell| {
        if cell.borrow().is_none() {
            let rpc_wasm_client = Arc::new(RpcWasmClient::new(|bytes| {
                static_muxio_emit_frame_bytes(&bytes)
            }));

            *cell.borrow_mut() = Some(rpc_wasm_client);
        }
    });
}

// TODO: Document
pub fn with_static_client_async<F, Fut, T>(f: F) -> Promise
where
    F: FnOnce(Arc<RpcWasmClient>) -> Fut + 'static,
    Fut: Future<Output = Result<T, String>> + 'static,
    T: Into<JsValue>,
{
    future_to_promise(async move {
        let maybe_client = MUXIO_STATIC_RPC_CLIENT_REF.with(|cell| cell.borrow().clone());

        if let Some(client) = maybe_client {
            match f(client).await {
                Ok(value) => Ok(value.into()),
                Err(e) => Err(JsValue::from_str(&format!("RPC error: {e}"))),
            }
        } else {
            Err(JsValue::from_str("RPC client not initialized"))
        }
    })
}
