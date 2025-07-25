use crate::RpcServiceCallerInterface;
use muxio::rpc::RpcRequest;
use muxio_rpc_service::{
    constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE, error::RpcServiceError,
    prebuffered::RpcMethodPrebuffered,
};
use std::{fmt::Debug, io};
use tracing::{self, instrument};

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
    ) -> Result<Self::Output, RpcServiceError>;
}

#[async_trait::async_trait]
impl<T> RpcCallPrebuffered for T
where
    T: RpcMethodPrebuffered + Send + Sync + 'static,
    T::Input: Send + 'static,
    T::Output: Send + 'static + Debug, // Add Debug trait bound here
{
    /// ### Large Argument Handling
    ///
    /// Due to underlying network transport limitations, a single RPC header frame
    /// cannot exceed a certain size (typically ~64KB). To handle arguments of any
    /// size, this method implements a "smart" transport strategy:
    ///
    /// 1.  **If the encoded arguments are small** (smaller than `DEFAULT_SERVICE_MAX_CHUNK_SIZE`),
    ///     they are sent in the `rpc_param_bytes` field of the request, which is part of
    ///     the initial header frame.
    ///
    /// 2.  **If the encoded arguments are large**, they cannot be sent in the header. Instead,
    ///     they are placed into the `rpc_prebuffered_payload_bytes` field. The underlying
    ///     `RpcDispatcher` will then automatically chunk this data and stream it as a
    ///     payload after the header.
    ///
    /// This ensures that RPC calls with large argument sets do not fail due to transport
    /// limitations, while still using the most efficient method for small arguments. The
    /// server-side `RpcServiceEndpointInterface` is designed with corresponding logic to
    ///  find the arguments in either location.
    #[instrument(skip(rpc_client, input))]
    async fn call<C: RpcServiceCallerInterface + Send + Sync>(
        rpc_client: &C,
        input: Self::Input,
    ) -> Result<Self::Output, RpcServiceError> {
        tracing::debug!("Starting for method ID: {}", T::METHOD_ID);
        let encoded_args = Self::encode_request(input)?;
        tracing::debug!("Arguments encoded ({} bytes).", encoded_args.len());

        let (req_param_bytes, req_payload_bytes) =
            if encoded_args.len() >= DEFAULT_SERVICE_MAX_CHUNK_SIZE {
                tracing::warn!("Arguments are large, using payload_bytes.");
                (None, Some(encoded_args))
            } else {
                tracing::debug!("Arguments are small, using param_bytes.");
                (Some(encoded_args), None)
            };

        let request = RpcRequest {
            rpc_method_id: Self::METHOD_ID,
            rpc_param_bytes: req_param_bytes,
            rpc_prebuffered_payload_bytes: req_payload_bytes,
            is_finalized: true, // IMPORTANT: All prebuffered requests should be considered finalized
        };
        tracing::debug!("RpcRequest created: {:?}", request);

        let decode_closure =
            |buffer: &[u8]| -> Result<Self::Output, io::Error> { Self::decode_response(buffer) };

        tracing::debug!("Calling `rpc_client.call_rpc_buffered`.");
        let (_encoder, nested_result) = rpc_client
            .call_rpc_buffered(request, decode_closure)
            .await?;
        tracing::debug!(
            "`rpc_client.call_rpc_buffered` returned. Nested result: {:?}",
            nested_result
        );

        match nested_result {
            Ok(decode_result) => {
                tracing::debug!("Unpacking nested_result: Ok. Decoding response.");
                decode_result.map_err(RpcServiceError::Transport)
            }
            Err(e) => {
                tracing::debug!("Unpacking nested_result: Err. Returning error: {:?}", e);
                Err(e)
            }
        }
    }
}
