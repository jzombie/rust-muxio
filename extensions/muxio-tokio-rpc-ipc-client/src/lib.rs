mod rpc_ipc_client;
pub use rpc_ipc_client::RpcIpcClient;

// Re-expose for simplicity
pub use muxio_rpc_service_caller::{
    RpcServiceCallerInterface, RpcTransportState, prebuffered::RpcCallPrebuffered,
};
