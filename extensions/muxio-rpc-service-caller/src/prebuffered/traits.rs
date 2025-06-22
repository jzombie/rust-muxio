use crate::{RpcServiceCallerInterface, error::RpcCallerError};
use muxio::rpc::RpcRequest;
use muxio_rpc_service::{
    constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE, prebuffered::RpcMethodPrebuffered,
};
use std::io;

// Add this use statement to bring `futures::StreamExt` into scope.
// This is needed for the `.next().await` call in the response buffering loop.
use futures::stream::StreamExt;

#[async_trait::async_trait]
pub trait RpcCallPrebuffered: RpcMethodPrebuffered + Sized + Send + Sync {
    /// Executes a pre-buffered RPC call.
    ///
    /// This is the primary method for making a simple request/response RPC call.
    /// It handles the encoding of arguments, the underlying network call, and the
    /// decoding of the response.
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
        let encoded_args = Self::encode_request(input)?;

        // ### Large Argument Handling
        //
        // Due to underlying network transport limitations, a single RPC header frame
        // cannot exceed a certain size (typically ~64KB). To handle arguments of any
        // size, this method implements a "smart" transport strategy:
        //
        // 1.  **If the encoded arguments are small** (smaller than `DEFAULT_SERVICE_MAX_CHUNK_SIZE`),
        //     they are sent in the `rpc_param_bytes` field of the request, which is part of
        //     the initial header frame.
        //
        // 2.  **If the encoded arguments are large**, they cannot be sent in the header. Instead,
        //     they are placed into the `rpc_prebuffered_payload_bytes` field. The underlying
        //     `RpcDispatcher` will then automatically chunk this data and stream it as a
        //     payload after the header.
        //
        // This ensures that RPC calls with large argument sets do not fail due to transport
        // limitations, while still using the most efficient method for small arguments. The
        // server-side `RpcServiceEndpointInterface` is designed with corresponding logic to
        //  find the arguments in either location.
        let (param_bytes, payload_bytes) = if encoded_args.len() >= DEFAULT_SERVICE_MAX_CHUNK_SIZE {
            (None, Some(encoded_args))
        } else {
            (Some(encoded_args), None)
        };

        let request = RpcRequest {
            rpc_method_id: Self::METHOD_ID,
            rpc_param_bytes: param_bytes,
            rpc_prebuffered_payload_bytes: payload_bytes,
            is_finalized: true, // IMPORTANT: All prebuffered requests should be considered finalized
        };

        // 1. Define the specific decode closure to pass to the generic helper.
        // Its job is to call our trait's `decode_response` method.
        let decode_closure =
            |buffer: &[u8]| -> Result<Self::Output, io::Error> { Self::decode_response(buffer) };

        // 2. Call the generic helper with our custom closure.
        let (_encoder, nested_result) = rpc_client
            .call_rpc_buffered(request, decode_closure)
            .await?;

        // 3. Unpack the nested `Result` and apply this trait's specific error handling.
        // The type of `nested_result` is: Result<Result<Self::Output, io::Error>, RpcCallerError>
        match nested_result {
            // The stream was successful, so now we check the result of our decode function.
            Ok(decode_result) => {
                // `decode_result` is the `Result<Self::Output, io::Error>` from our closure.
                // We can just return it directly.
                decode_result
            }
            // An error occurred during the stream itself (e.g., remote error).
            Err(rpc_error) => {
                // Here, we apply the specialized error formatting required by this trait.
                let error_message = match rpc_error {
                    RpcCallerError::RemoteError { payload } => {
                        format!(
                            "RPC call failed with remote error: {}",
                            String::from_utf8_lossy(&payload)
                        )
                    }
                    _ => rpc_error.to_string(),
                };
                Err(io::Error::other(error_message))
            }
        }
    }
}
