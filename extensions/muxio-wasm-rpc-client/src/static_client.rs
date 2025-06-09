use crate::RpcWasmClient;
use crate::muxio_emit_socket_frame_bytes;
use std::cell::RefCell;

thread_local! {
    pub static MUXIO_STATIC_RPC_CLIENT_REF: RefCell<Option<RpcWasmClient>> = RefCell::new(None);
}

/// Safe to call multiple times; only the first invocation has effect.
pub fn init_static_client() {
    MUXIO_STATIC_RPC_CLIENT_REF.with(|cell| {
        if cell.borrow().is_none() {
            let rpc_wasm_client = RpcWasmClient::new(|bytes| muxio_emit_socket_frame_bytes(&bytes));

            *cell.borrow_mut() = Some(rpc_wasm_client);
        }
    });
}
