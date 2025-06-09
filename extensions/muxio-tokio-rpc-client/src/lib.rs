mod rpc_client;
use muxio::rpc::RpcDispatcher;
use muxio_service_traits::RpcTransport;
pub use rpc_client::RpcClient;
use std::io;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

/// Native implementation of `RpcTransport` for `RpcClient`
#[async_trait::async_trait]
impl RpcTransport for RpcClient {
    type Dispatcher = RpcDispatcher<'static>;
    type Sender = UnboundedSender<WsMessage>;
    type Mutex<T: Send> = Mutex<T>;

    fn dispatcher(&self) -> Arc<Self::Mutex<Self::Dispatcher>> {
        self.dispatcher.clone()
    }

    fn sender(&self) -> Self::Sender {
        self.tx.clone()
    }

    /// Delegates the call to the actual `RpcClient::call_rpc` implementation.
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
        F: Fn(Vec<u8>) -> T + Send + Sync + 'static,
    {
        let (_dispatcher, result) = RpcClient::call_rpc(
            dispatcher,
            sender,
            method_id,
            payload,
            response_handler,
            is_finalized,
        )
        .await;

        Ok(result)
    }
}
