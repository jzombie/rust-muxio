pub mod service_definition;
use muxio_service_traits::{RpcRequestPrebuffered, RpcResponsePrebuffered, RpcTransport};
pub use service_definition::{Add, Mult};
use std::io;

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
    C: RpcTransport + Send + Sync,
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

/// Trait for types that represent callable prebuffered RPC methods.
///
/// This trait forms the final layer of abstraction, allowing downstream
/// users to write `T::call(&client, input)` without dealing with traits
/// or transport logic explicitly.
#[async_trait::async_trait]
pub trait RpcCallPrebuffered:
    RpcRequestPrebuffered + RpcResponsePrebuffered + Sized + Send + Sync
{
    async fn call<C: RpcTransport + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error>;
}

#[async_trait::async_trait]
impl RpcCallPrebuffered for Add {
    async fn call<C: RpcTransport + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error> {
        call_prebuffered_rpc::<Add, C>(rpc_client, input).await
    }
}

#[async_trait::async_trait]
impl RpcCallPrebuffered for Mult {
    async fn call<C: RpcTransport + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error> {
        call_prebuffered_rpc::<Mult, C>(rpc_client, input).await
    }
}
