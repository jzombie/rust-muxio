use crate::static_lib::get_static_client;

use super::MUXIO_STATIC_RPC_CLIENT_REF;
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// External JS function to send bytes from WASM to the network.
    fn static_muxio_write_bytes_uint8(data: Uint8Array);
}

/// Helper to convert Rust bytes to JS Uint8Array and send via the bridge.
pub(crate) fn static_muxio_write_bytes(bytes: &[u8]) {
    static_muxio_write_bytes_uint8(Uint8Array::from(bytes));
}

/// Entry point from JavaScript when binary data arrives from the network.
#[wasm_bindgen]
pub async fn static_muxio_read_bytes_uint8(inbound_data: Uint8Array) -> Result<(), JsValue> {
    let inbound_bytes = inbound_data.to_vec();

    // TODO: Use `get_static_client` for easier use
    let client_arc = MUXIO_STATIC_RPC_CLIENT_REF
        .with(|cell| cell.borrow().clone())
        .ok_or_else(|| JsValue::from_str("RPC client not initialized"))?;

    client_arc.read_bytes(&inbound_bytes).await;

    Ok(())
}

/// Call this from your JavaScript glue code when the WebSocket `onopen` event fires.
#[wasm_bindgen]
pub async fn static_muxio_handle_connect() -> Result<(), JsValue> {
    match get_static_client() {
        Some(static_client) => {
            static_client.handle_connect().await;
            Ok(())
        }
        None => Err("No registered static `RpcWasmClient`".into()),
    }
}

/// Call this from your JavaScript glue code when the WebSocket's `onclose` or `onerror` event fires.
#[wasm_bindgen]
pub async fn static_muxio_handle_disconnect() -> Result<(), JsValue> {
    match get_static_client() {
        Some(static_client) => {
            static_client.handle_disconnect().await;
            Ok(())
        }
        None => Err("No registered static `RpcWasmClient`".into()),
    }
}
