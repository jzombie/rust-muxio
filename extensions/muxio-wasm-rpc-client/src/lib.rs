// TODO: Remove
mod static_client;
pub use static_client::*;

// TODO: Remove
mod old_call_muxio;
pub use old_call_muxio::call_muxio;

mod socket_transport;
pub use socket_transport::*;

mod rpc_wasm_client;
pub use rpc_wasm_client::*;
