mod rpc_client;
use muxio::rpc::RpcDispatcher;
use muxio_service_traits::{RpcClientInterface, RpcRequestPrebuffered, RpcResponsePrebuffered};
pub use rpc_client::RpcClient;
use std::io;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

#[async_trait::async_trait]
impl RpcClientInterface for RpcClient {
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

// TODO: Refactor
/// Calls a prebuffered RPC method defined by the `RpcRequestPrebuffered` and
/// `RpcResponsePrebuffered` traits using a generic RPC transport.
///
/// This layer exists to decouple call-site logic from the encoding/decoding and
/// transport mechanics, allowing for easier composition and testability.
pub async fn call_prebuffered_rpc<T, C>(
    rpc_client: &C,
    input: T::Input,
) -> Result<T::Output, io::Error>
where
    T: RpcRequestPrebuffered + RpcResponsePrebuffered + Send + Sync + 'static,
    T::Output: Send + 'static,
    C: RpcClientInterface + Send + Sync,
    C::Dispatcher: Send,
{
    let dispatcher = rpc_client.dispatcher();
    let tx = rpc_client.sender();

    let transport_result = C::call_rpc(
        dispatcher,
        tx,
        <T as RpcRequestPrebuffered>::METHOD_ID,
        T::encode_request(input),
        T::decode_response,
        true,
    )
    .await?;

    // Error propagation is handled in two steps using two named variables:
    //
    // 1. `transport_result`: Result<Result<T::Output, io::Error>, io::Error>
    //    - This comes from the transport layer (e.g., socket communication).
    //    - The outer Result represents transport-level errors (e.g., channel closed).
    //
    // 2. `rpc_result`: T::Output
    //    - This unwraps the inner Result from `transport_result`.
    //    - If the remote RPC logic failed, this propagates that application-level error.
    let rpc_result = transport_result?;
    Ok(rpc_result)
}
