use std::future::Future;

#[async_trait::async_trait]
pub trait RpcServerInterface {
    async fn register_prebuffered<F, Fut>(
        &self,
        method_id: u64,
        handler: F,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: Fn(Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>
            + Send
            + 'static;
}
