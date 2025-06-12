use super::error::RpcServiceEndpointError;
use std::marker::Send;

// TODO: Refactor just like `client_interface`

/// Used so that servers (and optionally clients) can implement endpoint registration methods.
#[async_trait::async_trait]
pub trait RpcServiceEndpointInterface {
    async fn register_prebuffered<F, Fut>(
        &self,
        method_id: u64,
        handler: F,
    ) -> Result<(), RpcServiceEndpointError>
    where
        F: Fn(Vec<u8>) -> Fut + Send + Sync + 'static,
        // TODO: Use type alias
        Fut: Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>
            + Send
            + 'static;
}
