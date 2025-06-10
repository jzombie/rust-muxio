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
    fn decode_request(bytes: Vec<u8>) -> Result<Self::Input, io::Error>;

    /// Encodes the response value into a byte array.
    fn encode_response(output: Self::Output) -> Result<Vec<u8>, io::Error>;

    /// Decodes raw response bytes into a typed response struct or value.
    ///
    /// # Arguments
    /// * `bytes` - Serialized response payload.
    fn decode_response(bytes: Vec<u8>) -> Result<Self::Output, io::Error>;
}
