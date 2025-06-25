use super::static_muxio_write_bytes;
use crate::{RpcWasmClient, TransportState};
use js_sys::Promise;
use std::cell::RefCell;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

thread_local! {
    pub static MUXIO_STATIC_RPC_CLIENT_REF: RefCell<Option<Arc<RpcWasmClient>>> = const { RefCell::new(None) };
}

/// Initializes the global static RPC client if it has not been initialized.
///
/// This function is **idempotent**—calling it multiple times has no effect
/// after the first successful initialization.
///
/// Internally, it sets a global `MUXIO_STATIC_RPC_CLIENT_REF` with an
/// `Arc<RpcWasmClient>` instance that emits bytes via
/// `static_muxio_write_bytes`, which is bridged to JavaScript.
///
/// # Usage
/// This should be called once during WASM startup, typically from a JS
/// `init()` or entrypoint wrapper, **before** any RPC calls are issued.
pub fn init_static_client() {
    MUXIO_STATIC_RPC_CLIENT_REF.with(|cell| {
        if cell.borrow().is_none() {
            let rpc_wasm_client =
                Arc::new(RpcWasmClient::new(|bytes| static_muxio_write_bytes(&bytes)));

            *cell.borrow_mut() = Some(rpc_wasm_client);
        }
    });
}

/// Asynchronously executes a closure with the static `RpcWasmClient`, returning
/// the result as a JavaScript `Promise`.
///
/// This is the **primary way** to interact with the static RPC client from
/// exported async functions.
///
/// # Parameters
/// - `f`: A closure that receives the `Arc<RpcWasmClient>` and returns a
///   `Future` resolving to `Result<T, String>`, where `T: Into<JsValue>`.
///
/// # Returns
/// A JS `Promise` that resolves to `T` or rejects with an error string.
///
/// # Errors
/// If the static client has not been initialized via `init_static_client()`,
/// the promise will reject with `"RPC client not initialized"`.
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

/// Notifies the static Rust client of a transport state change.
/// This should be called from the JavaScript host environment (e.g., in
/// a WebSocket's `onopen` or `onclose` event listeners).
///
/// # JS-side State Mapping:
/// - `0`: Connecting
/// - `1`: Connected
/// - `2`: Disconnected
#[wasm_bindgen]
pub fn notify_static_client_transport_state_change(state_code: u8) -> Result<(), JsValue> {
    let state = match state_code {
        0 => TransportState::Connecting,
        1 => TransportState::Connected,
        2 => TransportState::Disconnected,
        _ => return Err(JsValue::from_str("Invalid state code provided.")),
    };

    MUXIO_STATIC_RPC_CLIENT_REF.with(|cell| {
        if let Some(client) = cell.borrow().as_ref() {
            // Acquire the lock and invoke the handler if it's set.
            if let Some(handler) = client.state_change_handler().lock().unwrap().as_ref() {
                handler(state);
            }
        }
        // It's not an error if the client isn't initialized or no handler is set.
        Ok(())
    })
}
