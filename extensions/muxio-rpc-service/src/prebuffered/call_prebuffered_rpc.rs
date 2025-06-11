use crate::{RpcClientInterface, prebuffered::RpcMethodPrebuffered};
use std::io;

// TODO: Remove
/// Performs a one-shot (pre-buffered) RPC call using a method that conforms to
/// the `RpcMethodPrebuffered` interface.
///
/// This function is designed for RPC methods where the entire request payload
/// is available upfront (i.e., prebuffered). It handles the full lifecycle of:
/// - Encoding the input
/// - Sending the request via the generic transport
/// - Decoding the response
///
/// This abstraction allows call sites to remain agnostic of the transport
/// implementation and serialization format, making it easier to compose,
/// test, or mock RPC workflows.
pub async fn call_prebuffered_rpc<T, C>(
    rpc_client: &C,
    input: T::Input,
) -> Result<T::Output, io::Error>
where
    T: RpcMethodPrebuffered + Send + Sync + 'static,
    T::Input: Send + 'static,
    T::Output: Send + 'static,
    C: RpcClientInterface + Send + Sync,
{
    let (_, rpc_result) = rpc_client
        .call_rpc(
            T::METHOD_ID,
            &T::encode_request(input)?,
            T::decode_response,
            true,
        )
        .await?;

    rpc_result?
}
