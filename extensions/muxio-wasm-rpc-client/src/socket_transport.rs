use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;
use web_sys::console;

// use super::MUXIO_CLIENT_DISPATCHER_REF;

#[wasm_bindgen]
extern "C" {
    /// JavaScript-exposed binding for sending a frame over the socket transport.
    ///
    /// This function must be implemented in the JavaScript host environment.
    /// It is called internally by the Rust runtime to send encoded RPC frame data.
    fn muxio_emit_socket_frame_uint8(data: Uint8Array);
}

/// Sends a raw byte slice over the socket transport using the JS-bound emitter.
///
/// Converts the given Rust `&[u8]` into a `Uint8Array` and passes it to
/// the JavaScript `muxio_emit_socket_frame_uint8` function.
pub fn muxio_emit_socket_frame_bytes(bytes: &[u8]) {
    muxio_emit_socket_frame_uint8(Uint8Array::from(bytes));
}

// TODO: Refactor accordingly
// /// Handles an inbound socket frame received from the JavaScript layer.
// ///
// /// This function should be called by JS whenever a new binary message is received.
// /// It decodes the incoming `Uint8Array` and passes it to the internal RPC dispatcher.
// #[wasm_bindgen]
// pub fn muxio_receive_socket_frame_uint8(inbound_data: Uint8Array) -> Result<(), JsValue> {
//     // Convert Uint8Array to Vec<u8>
//     let inbound_bytes = inbound_data.to_vec();

//     MUXIO_CLIENT_DISPATCHER_REF.with(|cell| {
//         if let Some(client_dispatcher) = cell.borrow_mut().as_mut() {
//             client_dispatcher.receive_bytes(&inbound_bytes).unwrap();
//         } else {
//             console::error_1(&"Dispatcher not initialized".into());
//         }
//     });

//     Ok(())
// }

/// JS-facing hook for handling socket connection establishment.
///
/// Can be called from JS when a connection to the backend socket is opened.
///
/// This currently only logs to the console, but can be expanded to
/// set internal flags or trigger handshake logic.
#[wasm_bindgen]
pub fn handle_muxio_socket_connect() -> Result<(), JsValue> {
    console::debug_1(&"Handle socket connect...".into());
    Ok(())
}

/// JS-facing hook for handling socket disconnection events.
///
/// Can be called from JS when the socket connection is lost or closed.
///
/// This currently only logs to the console, but can be expanded to
/// support reconnection logic, exponential backoff, etc.
#[wasm_bindgen]
pub fn handle_muxio_socket_disconnect() -> Result<(), JsValue> {
    console::debug_1(&"Handle socket disconnect...".into());
    Ok(())
}
