use super::RpcClientInterface;
use std::io;

// These are optional helper traits that define a convention for encoding and
// decoding RPC method data using pre-buffered (i.e., fully materialized) payloads.
//
// These traits are not used by the core Muxio framework directly — it is up to
// the consuming application to adopt them if desired. They provide a structured,
// ergonomic way to couple method metadata (such as `METHOD_ID`) with
// serialization logic in a single location.
//
// These traits assume that the transport has already buffered the complete
// request or response body, making them unsuitable for streaming scenarios.
// In cases requiring incremental transmission, alternative traits or interfaces
// should be used instead.

/// Trait for types that represent callable prebuffered RPC methods.
///
/// This trait forms the final layer of abstraction, allowing downstream
/// users to write `T::call(&client, input)` without dealing with traits
/// or transport logic explicitly.
#[async_trait::async_trait]
pub trait RpcCallPrebuffered: RpcMethodPrebuffered + Sized + Send + Sync {
    async fn call<C: RpcClientInterface + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error>;
}

pub trait RpcMethodPrebuffered {
    /// A unique identifier for the RPC method.
    const METHOD_ID: u64; // TODO: Make helper for this to help avoid numeric collisions

    /// The high-level input type expected by the request encoder (e.g., `Vec<f64>`).
    type Input;

    /// The high-level output type returned from the response encoder (e.g., `f64`).
    type Output;

    /// Encodes the request into a byte array.
    fn encode_request(input: Self::Input) -> Result<Vec<u8>, io::Error>;

    /// Decodes raw request bytes into a typed request struct.
    ///
    /// # Arguments
    /// * `bytes` - Serialized request payload.
    fn decode_request(bytes: &[u8]) -> Result<Self::Input, io::Error>;

    /// Encodes the response value into a byte array.
    fn encode_response(output: Self::Output) -> Result<Vec<u8>, io::Error>;

    /// Decodes raw response bytes into a typed response struct or value.
    ///
    /// # Arguments
    /// * `bytes` - Serialized response payload.
    fn decode_response(bytes: &[u8]) -> Result<Self::Output, io::Error>;
}

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
