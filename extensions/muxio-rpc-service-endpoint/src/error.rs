use muxio::frame::{FrameDecodeError, FrameEncodeError};
use muxio_rpc_service::error::RpcServiceErrorPayload;
use std::fmt;

/// The special error type that a handler should return to send a
/// structured error to the caller.
#[derive(Debug)]
pub struct RpcServiceEndointHandlerError(pub RpcServiceErrorPayload);

impl fmt::Display for RpcServiceEndointHandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Handler failed with code {:?}: {}",
            self.0.code, self.0.message
        )
    }
}

impl std::error::Error for RpcServiceEndointHandlerError {}

/// Represents errors that can occur within the endpoint's own logic.
#[derive(Debug)]
pub enum RpcServiceEndpointError {
    Decode(FrameDecodeError),
    Encode(FrameEncodeError),
    Handler(Box<dyn std::error::Error + Send + Sync>),
}

impl From<FrameDecodeError> for RpcServiceEndpointError {
    fn from(err: FrameDecodeError) -> Self {
        RpcServiceEndpointError::Decode(err)
    }
}

impl From<FrameEncodeError> for RpcServiceEndpointError {
    fn from(err: FrameEncodeError) -> Self {
        RpcServiceEndpointError::Encode(err)
    }
}
