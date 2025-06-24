pub use muxio_rpc_service_caller::RpcServiceCallerInterface;
mod rpc_client;
pub use rpc_client::RpcClient;

// Re-expose for simplicity
pub use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
