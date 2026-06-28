mod ipc_client;
pub use ipc_client::IpcClient;

// Re-expose for simplicity
pub use muxio_rpc_service_caller::{
    RpcServiceCallerInterface, RpcTransportState, prebuffered::RpcCallPrebuffered,
};
