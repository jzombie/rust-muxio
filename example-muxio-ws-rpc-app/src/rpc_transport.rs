use std::io;
use std::sync::Arc;

// TODO: Refactor
/// Abstracts the RPC transport mechanism.
///
/// This trait enables usage of any client type that implements the required
/// RPC communication functionality. It allows runtime selection and injection
/// of client implementations across different environments (e.g. native vs wasm).
#[async_trait::async_trait]
pub trait RpcTransport {
    /// Must be `RpcDispatcher<'static>` or compatible with it.
    type Dispatcher: Send + 'static;

    /// Abstract sender type responsible for outgoing messages.
    type Sender;

    /// Mutex abstraction that can be customized for different runtimes.
    type Mutex<T: Send>: Send + Sync;

    /// Returns a shared reference to the dispatcher.
    fn dispatcher(&self) -> Arc<Self::Mutex<Self::Dispatcher>>;

    /// Returns the sender.
    fn sender(&self) -> Self::Sender;

    /// Generalized RPC call that transports a request.
    async fn call_rpc<T, F>(
        dispatcher: Arc<Self::Mutex<Self::Dispatcher>>,
        sender: Self::Sender,
        method_id: u64,
        payload: Vec<u8>,
        response_handler: F,
        is_finalized: bool,
    ) -> Result<T, io::Error>
    where
        T: Send + 'static,
        F: Fn(Vec<u8>) -> T + Send + Sync + 'static;
}
