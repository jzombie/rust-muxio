use muxio::rpc::rpc_internals::RpcStreamEncoder;
use std::io;

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
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn for<'a> FnMut(&'a [u8]) + Send + 'static>>,
            T,
        ),
        io::Error,
    >
    where
        T: Send + 'static,
        F: Fn(Vec<u8>) -> T + Send + Sync + 'static;
}
