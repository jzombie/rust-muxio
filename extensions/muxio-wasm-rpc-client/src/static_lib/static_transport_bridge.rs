use js_sys::Uint8Array;
use muxio_rpc_service::RpcClientInterface;
use wasm_bindgen::prelude::*;

use super::MUXIO_STATIC_RPC_CLIENT_REF;

#[wasm_bindgen]
extern "C" {
    /// RpcClient => Network
    ///
    /// Invoked by the RpcClient, this external JavaScript function is used to
    /// send raw bytes over the wire.
    ///
    /// This must be implemented in JavaScript and made available in the WASM
    /// runtime context.
    ///
    /// Called internally by `RpcWasmClient` emit callbacks when transmitting
    /// outbound frames.
    ///
    /// # Signature (expected in JS)
    /// ```js
    /// globalThis.static_muxio_write_bytes_uint8 = (uint8Array) => {
    ///     socket.send(uint8Array);
    /// };
    /// ```
    fn static_muxio_write_bytes_uint8(data: Uint8Array);
}

/// Forwards a Rust byte slice to JavaScript as a `Uint8Array` via the
/// `static_muxio_write_bytes_uint8` bridge.
///
/// This is typically used as the `emit_callback` in `RpcWasmClient` and
/// is not intended to be called manually.
pub(crate) fn static_muxio_write_bytes(bytes: &[u8]) {
    static_muxio_write_bytes_uint8(Uint8Array::from(bytes));
}

/// Network => RpcClient
///
/// Called from JavaScript when inbound socket data arrives as a `Uint8Array`.
///
/// This function deserializes the byte buffer and feeds it to the static
/// `RpcWasmClient`'s dispatcher for decoding and handling.
///
/// # Parameters
/// - `inbound_data`: A `Uint8Array` representing binary-encoded Muxio frames.
///
/// # Returns
/// - `Ok(())` on success
/// - `Err(JsValue)` if the client was not initialized
///
/// # Usage (JavaScript)
/// ```js
/// socket.onmessage = (e) => {
///   static_muxio_read_bytes_uint8(new Uint8Array(e.data));
/// };
/// ```
#[wasm_bindgen]
pub fn static_muxio_read_bytes_uint8(inbound_data: Uint8Array) -> Result<(), JsValue> {
    // Convert Uint8Array to Vec<u8>
    let inbound_bytes = inbound_data.to_vec();

    MUXIO_STATIC_RPC_CLIENT_REF.with(|cell| {
        if let Some(rpc_wasm_client) = cell.borrow_mut().as_mut() {
            rpc_wasm_client
                .get_dispatcher()
                .lock()
                .unwrap()
                .read_bytes(&inbound_bytes)
                .unwrap();
        } else {
            // TODO: Use tracing instead?
            panic!("Dispatcher not initialized");
        }
    });

    Ok(())
}
