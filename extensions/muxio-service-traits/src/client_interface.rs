use std::io;
use std::sync::Arc;

// TODO: Put behind a feature flag for "rpc-client"?
/// This trait enables usage of any client type that implements the required
/// RPC communication functionality. It allows runtime selection and injection
/// of client implementations across different environments (e.g. native vs wasm).
#[async_trait::async_trait]
pub trait RpcClientInterface {
    type Client;
    type Sender;
    type Mutex<T: Send>: Send + Sync;

    async fn call_rpc<T, F>(
        &self,
        method_id: u64,
        payload: Vec<u8>,
        response_handler: F,
        is_finalized: bool,
    ) -> Result<T, io::Error>
    where
        T: Send + 'static,
        F: Fn(Vec<u8>) -> T + Send + Sync + 'static;
}
