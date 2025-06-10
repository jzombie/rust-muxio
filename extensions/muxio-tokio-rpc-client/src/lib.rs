mod rpc_client;
use muxio::rpc::rpc_internals::RpcStreamEncoder;
use muxio_rpc_service::{RpcClientInterface};
pub use rpc_client::RpcClient;
use std::io;

// TODO: Use feature flag for tokio vs. non-tokio here?

// TODO: Move into `RpcClient`
#[async_trait::async_trait]
impl RpcClientInterface for RpcClient {
    async fn call_rpc<T, F>(
        &self,
        method_id: u64,
        payload: Vec<u8>,
        response_handler: F,
        is_finalized: bool,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn for<'a> FnMut(&'a [u8]) + Send + 'static>>,
            T,
        ),
        io::Error,
    >
    where
        T: Send + 'static,
        F: Fn(Vec<u8>) -> T + Send + Sync + 'static,
    {
        let transport_result = self
            .call_rpc(method_id, payload, response_handler, is_finalized)
            .await;

        Ok(transport_result)
    }
}


