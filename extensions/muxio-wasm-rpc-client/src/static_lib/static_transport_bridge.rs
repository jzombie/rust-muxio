use super::MUXIO_STATIC_RPC_CLIENT_REF;
use futures::future::join_all;
use js_sys::Uint8Array;
use muxio::rpc::RpcRequest;
use muxio_rpc_service::constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE;
use muxio_rpc_service_caller::RpcServiceCallerInterface;
use muxio_rpc_service_endpoint::RpcServiceEndpointInterface;
use muxio_rpc_service_endpoint::process_single_prebuffered_request;
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

    let client_arc = MUXIO_STATIC_RPC_CLIENT_REF
        .with(|cell| cell.borrow().clone())
        .ok_or_else(|| JsValue::from_str("RPC client not initialized"))?;

    let dispatcher_arc = client_arc.get_dispatcher();
    let endpoint_arc = client_arc.get_endpoint();
    let emit_fn_arc = client_arc.get_emit_fn();

    spawn_local(async move {
        // Stage 1: Synchronous Reading from Dispatcher (lock briefly held)
        let mut requests_to_process: Vec<(u32, RpcRequest)> = Vec::new();
        {
            // TODO: This might can be replaced with `process_incoming_bytes`
            let mut dispatcher_guard = dispatcher_arc.lock().await;
            match dispatcher_guard.read_bytes(&inbound_bytes) {
                Ok(request_ids) => {
                    for id in request_ids {
                        if dispatcher_guard
                            .is_rpc_request_finalized(id)
                            .unwrap_or(false)
                        {
                            if let Some(req) = dispatcher_guard.delete_rpc_request(id) {
                                requests_to_process.push((id, req));
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "WASM static_read_bytes: Dispatcher read_bytes error: {:?}",
                        e
                    );
                    return;
                }
            }
        } // `dispatcher_guard` is dropped here.

        // Stage 2: Asynchronous Processing of Requests (NO dispatcher lock held)
        let mut response_futures = Vec::new();
        let handlers_arc = endpoint_arc.get_prebuffered_handlers(); // Get the endpoint's handlers lock

        for (request_id, request) in requests_to_process {
            let handlers_arc_clone = handlers_arc.clone();
            let handler_context = (); // Context is () for WASM client

            // Call the new common utility function!
            let future = process_single_prebuffered_request(
                handlers_arc_clone,
                handler_context,
                request_id,
                request,
            );
            response_futures.push(future);
        }

        let responses_to_send = join_all(response_futures).await;

        // Stage 3: Synchronous Sending of Responses (lock briefly re-acquired)
        {
            let mut dispatcher_guard = dispatcher_arc.lock().await;
            for response in responses_to_send {
                let emit_fn_clone_for_respond = emit_fn_arc.clone();
                let _ = dispatcher_guard.respond(
                    response,
                    DEFAULT_SERVICE_MAX_CHUNK_SIZE,
                    move |chunk: &[u8]| {
                        emit_fn_clone_for_respond(chunk.to_vec());
                    },
                );
            }
        }
    });

    Ok(())
}
