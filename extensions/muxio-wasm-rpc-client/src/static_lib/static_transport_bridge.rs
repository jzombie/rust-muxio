use super::MUXIO_STATIC_RPC_CLIENT_REF;
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

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
pub fn static_muxio_read_bytes_uint8(inbound_data: Uint8Array) -> Result<(), JsValue> {
    let inbound_bytes = inbound_data.to_vec();

    // TODO: Use `get_static_client` for easier use
    let client_arc = MUXIO_STATIC_RPC_CLIENT_REF
        .with(|cell| cell.borrow().clone())
        .ok_or_else(|| JsValue::from_str("RPC client not initialized"))?;

    spawn_local(async move {
        client_arc.read_bytes(&inbound_bytes).await;
    });

    Ok(())
}

// TODO: Expose `static_muxio_handle_connect`
// TODO: Expose `static_muxio_handle_disconnect`
