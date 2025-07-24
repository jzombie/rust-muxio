use crate::RpcServiceCallerInterface;
use muxio::rpc::RpcRequest;
use muxio_rpc_service::{
    constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE, error::RpcServiceError,
    prebuffered::RpcMethodPrebuffered,
};
use std::fmt::Debug;
use std::io; // Import Debug trait

#[async_trait::async_trait(?Send)]
pub trait RpcCallPrebuffered: RpcMethodPrebuffered + Sized + Send + Sync {
    /// Executes a pre-buffered RPC call.
    ///
    /// This is the primary method for making a simple request/response RPC call.
    /// It handles the encoding of arguments, the underlying network call, and the
    /// decoding of the response.
    async fn call<C: RpcServiceCallerInterface + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, RpcServiceError>;
}

#[async_trait::async_trait(?Send)]
impl<T> RpcCallPrebuffered for T
where
    T: RpcMethodPrebuffered + Send + Sync + 'static,
    T::Input: Send + 'static,
    T::Output: Send + 'static + Debug, // Add Debug trait bound here
{
    async fn call<C: RpcServiceCallerInterface + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, RpcServiceError> {
        println!(
            "[RpcCallPrebuffered::call] Starting for method ID: {}",
            T::METHOD_ID
        );
        let encoded_args = Self::encode_request(input)?;
        println!(
            "[RpcCallPrebuffered::call] Arguments encoded ({} bytes).",
            encoded_args.len()
        );

        let (param_bytes, payload_bytes) = if encoded_args.len() >= DEFAULT_SERVICE_MAX_CHUNK_SIZE {
            println!("[RpcCallPrebuffered::call] Arguments are large, using payload_bytes.");
            (None, Some(encoded_args))
        } else {
            println!("[RpcCallPrebuffered::call] Arguments are small, using param_bytes.");
            (Some(encoded_args), None)
        };

        let request = RpcRequest {
            rpc_method_id: Self::METHOD_ID,
            rpc_param_bytes: param_bytes,
            rpc_prebuffered_payload_bytes: payload_bytes,
            is_finalized: true, // IMPORTANT: All prebuffered requests should be considered finalized
        };
        println!(
            "[RpcCallPrebuffered::call] RpcRequest created: {:?}",
            request
        );

        let decode_closure =
            |buffer: &[u8]| -> Result<Self::Output, io::Error> { Self::decode_response(buffer) };

        println!("[RpcCallPrebuffered::call] Calling rpc_client.call_rpc_buffered.");
        let (_encoder, nested_result) = rpc_client
            .call_rpc_buffered(request, decode_closure)
            .await?;
        println!(
            "[RpcCallPrebuffered::call] rpc_client.call_rpc_buffered returned. Nested result: {:?}",
            nested_result
        );

        match nested_result {
            Ok(decode_result) => {
                println!(
                    "[RpcCallPrebuffered::call] Unpacking nested_result: Ok. Decoding response."
                );
                decode_result.map_err(RpcServiceError::Transport)
            }
            Err(e) => {
                println!(
                    "[RpcCallPrebuffered::call] Unpacking nested_result: Err. Returning error: {:?}",
                    e
                );
                Err(e)
            }
        }
    }
}
