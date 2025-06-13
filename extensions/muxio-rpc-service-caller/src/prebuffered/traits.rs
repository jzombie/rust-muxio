use crate::RpcServiceCallerInterface;
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
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
    async fn call<C: RpcServiceCallerInterface + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error>;
}

#[async_trait::async_trait]
impl<T> RpcCallPrebuffered for T
where
    T: RpcMethodPrebuffered + Send + Sync + 'static,
    T::Input: Send + 'static,
    T::Output: Send + 'static,
{
    async fn call<C: RpcServiceCallerInterface + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, io::Error> {
        let encoded = Self::encode_request(input)?;
        let (_, inner) = rpc_client
            .call_rpc_buffered(Self::METHOD_ID, &encoded, Self::decode_response, true)
            .await?;

        // FIX: The `inner` value is a `Result<Result<Output, _>, Vec<u8>>`.
        // We match on it to handle the custom error payload or the success/decode error.
        match inner {
            // `decode_result` is the `Result<Self::Output, io::Error>` from `decode_response`.
            // We can return it directly as it matches our function's signature.
            Ok(decode_result) => decode_result,
            // The call failed with a remote error payload.
            Err(error_payload) => {
                // Convert the byte payload into a user-friendly string message.
                let error_message = String::from_utf8_lossy(&error_payload);
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("RPC call failed with remote error: {}", error_message),
                ))
            }
        }
    }
}
