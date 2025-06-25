mod rpc_wasm_client;
pub use rpc_wasm_client::*;

pub mod static_lib;

// Re-expose for simplicity
pub use muxio_rpc_service_caller::{
    RpcServiceCallerInterface, TransportState, prebuffered::RpcCallPrebuffered,
};
