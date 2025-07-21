use js_sys::Uint8Array;
use muxio_rpc_service_caller::RpcServiceCallerInterface;
use muxio_rpc_service_endpoint::RpcServiceEndpointInterface;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

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
///    static_muxio_read_bytes_uint8(new Uint8Array(e.data));
/// };
/// ```
#[wasm_bindgen]
pub fn static_muxio_read_bytes_uint8(inbound_data: Uint8Array) -> Result<(), JsValue> {
    // Convert Uint8Array to Vec<u8>
    let inbound_bytes = inbound_data.to_vec();

    // Get a clone of the Arc<RpcWasmClient> to move into the async block
    let client_arc = MUXIO_STATIC_RPC_CLIENT_REF
        .with(|cell| cell.borrow().clone())
        .ok_or_else(|| JsValue::from_str("RPC client not initialized"))?;

    // Spawn an async task to process the bytes
    spawn_local(async move {
        let dispatcher_arc = client_arc.get_dispatcher();
        let endpoint_arc = client_arc.get_endpoint();
        let emit_fn = client_arc.get_emit_fn(); // Get the client's emit function for responses

        // Lock the dispatcher once for this async operation
        let mut dispatcher_guard = dispatcher_arc.lock().expect("Dispatcher mutex poisoned");

        // The endpoint's read_bytes method will process incoming requests
        // AND use the dispatcher for responses if the handler calls respond().
        // It also uses the provided on_emit for sending those responses.
        if let Err(e) = endpoint_arc
            .read_bytes(
                &mut dispatcher_guard,
                (), // No context needed for the endpoint in this case
                &inbound_bytes,
                // The on_emit callback for the endpoint should use the client's emit_fn
                // to send response chunks back over the WebSocket.
                move |chunk: &[u8]| {
                    emit_fn(chunk.to_vec());
                },
            )
            .await
        {
            tracing::error!(
                "WASM static_read_bytes: Error processing inbound RPC bytes: {:?}",
                e
            );
        }
    });

    Ok(())
}
