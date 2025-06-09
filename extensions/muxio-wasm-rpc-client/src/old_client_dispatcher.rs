// TODO: Remove

use crate::RpcWasmClient;
use crate::muxio_emit_socket_frame_bytes;
use muxio::rpc::RpcDispatcher;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

thread_local! {
    /// A thread-local, optionally-initialized client dispatcher used to manage
    /// outbound RPC calls and handle inbound stream events.
    ///
    /// This is the central coordination point for client-side RPC traffic
    /// in the WebAssembly runtime. It is accessed via `.with(...)` to ensure
    /// thread-local safety.
    ///
    /// Initialization must be explicitly triggered via `init_client_dispatcher()`.
    ///
    /// The `'static` lifetime is used due to the absence of scoped lifetimes
    /// in the WASM module and ensures compatibility with long-lived global
    /// callbacks like event streams from JS.
    pub static MUXIO_CLIENT_DISPATCHER_REF: RefCell<Option<Arc<Mutex<RpcDispatcher<'static>>>>> = RefCell::new(None);
}

/// Initializes the global MUXIO client dispatcher if it has not already
/// been set.
///
/// This function must be called early (e.g., from `#[wasm_bindgen(start)]`)
/// to ensure that the dispatcher is available to handle both outgoing calls
/// and incoming socket frames.
///
/// Safe to call multiple times; only the first invocation has effect.
pub fn init_client_dispatcher() {
    MUXIO_CLIENT_DISPATCHER_REF.with(|cell| {
        if cell.borrow().is_none() {
            let rpc_wasm_client = RpcWasmClient::new(|bytes| muxio_emit_socket_frame_bytes(&bytes));

            let client_dispatcher = rpc_wasm_client.dispatcher;
            *cell.borrow_mut() = Some(client_dispatcher);
        }
    });
}
