use futures::channel::mpsc::Receiver;
use muxio::rpc::{
    RpcDispatcher,
    rpc_internals::{RpcStreamEncoder, rpc_trait::RpcEmit},
};
use std::{io, sync::Arc};

// TODO: Rename to RpcServiceCallerInterface
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
    // Typically will be either `std::sync::Mutex` or `tokio::sync::Mutex`
    type DispatcherMutex<Dispatcher>;

    fn get_dispatcher(&self) -> Arc<Self::DispatcherMutex<RpcDispatcher<'static>>>;

    async fn call_rpc_streaming(
        &self,
        method_id: u64,
        payload: &[u8],
        is_finalized: bool,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            Receiver<Vec<u8>>, // streamed response chunks
        ),
        io::Error,
    >;

    /// Buffered interface: collects all response chunks before decoding.
    async fn call_rpc_buffered<T, F>(
        &self,
        method_id: u64,
        payload: &[u8],
        decode: F,
        is_finalized: bool,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            Result<T, io::Error>,
        ),
        io::Error,
    >
    where
        T: Send + 'static,
        F: Fn(&[u8]) -> T + Send + Sync + 'static;
}
