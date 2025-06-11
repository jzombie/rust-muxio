use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;
use web_sys::console;

use super::MUXIO_STATIC_RPC_CLIENT_REF;

#[wasm_bindgen]
extern "C" {
    // Internally called when the RPC client has data to share over the network.
    fn static_muxio_write_bytes_uint8(data: Uint8Array);
}

// Internally called to convert bytes to `Uint8Array` for JavaScript.
pub(crate) fn static_muxio_write_bytes(bytes: &[u8]) {
    static_muxio_write_bytes_uint8(Uint8Array::from(bytes));
}

// Called from JS when network data is available to the client.
#[wasm_bindgen]
pub fn static_muxio_read_bytes_uint8(inbound_data: Uint8Array) -> Result<(), JsValue> {
    // Convert Uint8Array to Vec<u8>
    let inbound_bytes = inbound_data.to_vec();

    MUXIO_STATIC_RPC_CLIENT_REF.with(|cell| {
        if let Some(rpc_wasm_client) = cell.borrow_mut().as_mut() {
            rpc_wasm_client
                .dispatcher
                .lock()
                .unwrap()
                .read_bytes(&inbound_bytes)
                .unwrap();
        } else {
            console::error_1(&"Dispatcher not initialized".into());
        }
    });

    Ok(())
}
