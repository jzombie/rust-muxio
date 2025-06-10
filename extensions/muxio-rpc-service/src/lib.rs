mod prebuffered;
pub use prebuffered::*;
mod client_interface;
pub use client_interface::*;
use std::io;
pub mod constants;
pub use constants::*;
mod macros;
pub use macros::*;

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
            T::encode_request(input)?,
            T::decode_response,
            true,
        )
        .await?;

    rpc_result
}

/// Blanket implementation of the `RpcCallPrebuffered` trait for any type
/// that also implements `RpcMethodPrebuffered`.
///
/// This enables `.call()` usage on any RPC method type without requiring
/// a manual implementation for each one.
///
/// This reduces boilerplate and enforces consistency across your service
/// method definitions.
#[async_trait::async_trait]
impl<T> RpcCallPrebuffered for T
where
    T: RpcMethodPrebuffered + Send + Sync + 'static,
    T::Input: Send + 'static,
    T::Output: Send + 'static,
{
    async fn call<C>(rpc_client: &C, input: Self::Input) -> Result<Self::Output, io::Error>
    where
        C: RpcClientInterface + Send + Sync,
    {
        call_prebuffered_rpc::<T, C>(rpc_client, input).await
    }
}
