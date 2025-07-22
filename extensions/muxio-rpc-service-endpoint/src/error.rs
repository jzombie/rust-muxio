use muxio::frame::{FrameDecodeError, FrameEncodeError};
use muxio_rpc_service::error::RpcServiceErrorPayload;
use std::fmt;

// TODO: Remove
/// The special error type that a handler should return to send a
/// structured error to the caller.
#[derive(Debug)]
pub struct HandlerError(pub RpcServiceErrorPayload);

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Handler failed with code {:?}: {}",
            self.0.code, self.0.message
        )
    }
}

impl std::error::Error for HandlerError {}

// TODO: Only public in crate
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
