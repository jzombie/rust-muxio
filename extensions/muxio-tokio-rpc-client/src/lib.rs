mod rpc_client;
use muxio::rpc::rpc_internals::RpcStreamEncoder;
use muxio_service_traits::{RpcClientInterface, RpcMethodPrebuffered};
pub use rpc_client::RpcClient;
use std::io;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

// TODO: Use feature flag for tokio vs. non-tokio here?

// TODO: Move into `RpcClient`
#[async_trait::async_trait]
impl RpcClientInterface for RpcClient {
    type Client = RpcClient;
    type Sender = UnboundedSender<WsMessage>;
    type Mutex<T: Send> = Mutex<T>;

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

// TODO: Refactor (and dedupe)
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
    T: RpcMethodPrebuffered + Send + Sync + 'static,
    T::Output: Send + 'static,
    C: RpcClientInterface + Send + Sync,
    // C::Dispatcher: Send,
{
    // let dispatcher = rpc_client.dispatcher();
    // let tx = rpc_client.sender();

    let transport_result = rpc_client
        .call_rpc(
            <T as RpcMethodPrebuffered>::METHOD_ID,
            T::encode_request(input),
            T::decode_response,
            true,
        )
        .await?;

    let (_, rpc_result) = transport_result;

    rpc_result
}
