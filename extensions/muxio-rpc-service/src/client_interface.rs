use muxio::rpc::rpc_internals::RpcStreamEncoder;

/// A transport-agnostic RPC client interface supporting both
/// one-shot (pre-buffered) and streaming request workflows.
///
/// This trait enables making an RPC call by sending an encoded request payload
/// and receiving a decoded result. Additionally, it returns a writable
/// `RpcStreamEncoder` which allows further streaming of payload fragments,
/// if applicable.
///
/// This makes the interface suitable for both:
/// - **Prebuffered RPC calls** (single payload, immediate response)
/// - **Streamed RPC calls** (multiple payload fragments, possibly long-lived)
///
#[async_trait::async_trait]
pub trait RpcClientInterface {
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
        std::io::Error,
    >
    where
        T: Send + 'static,
        F: Fn(Vec<u8>) -> T + Send + Sync + 'static;
}
