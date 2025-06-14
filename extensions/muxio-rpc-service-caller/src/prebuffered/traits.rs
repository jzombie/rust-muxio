use crate::{RpcServiceCallerInterface, error::RpcCallerError};
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use std::io;

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
        let (_, inner_result) = rpc_client
            .call_rpc_buffered(Self::METHOD_ID, &encoded, Self::decode_response, true)
            .await?;

        // FIX: Handle the different variants of the `RpcCallerError` enum.
        match inner_result {
            // The `output` variable is already the `Result` from the `decode_response`
            // function, so we can return it directly.
            Ok(output) => output,

            // The RPC call failed. We now create a descriptive `io::Error`.
            Err(rpc_error) => {
                let error_message = match rpc_error {
                    // If the server sent back a specific error payload, include it.
                    RpcCallerError::RemoteError { payload } => {
                        format!(
                            "RPC call failed with remote error: {}",
                            String::from_utf8_lossy(&payload)
                        )
                    }
                    // For other errors (like system errors or I/O issues), just use their string representation.
                    _ => rpc_error.to_string(),
                };
                Err(io::Error::other(error_message))
            }
        }
    }
}
