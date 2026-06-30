use muxio_core::frame::{FrameDecodeError, FrameEncodeError};
use muxio_rpc_service::error::{RpcServiceErrorCode, RpcServiceErrorPayload};
use std::fmt;
use std::io;

/// The special error type that a handler should return to send a
/// structured error to the caller.
#[derive(Debug)]
pub struct RpcServiceEndpointHandlerError(pub RpcServiceErrorPayload);

impl fmt::Display for RpcServiceEndpointHandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Handler failed with code {:?}: {}",
            self.0.code, self.0.message
        )
    }
}

impl std::error::Error for RpcServiceEndpointHandlerError {}

/// Represents errors that can occur within the endpoint's own logic.
#[derive(Debug)]
pub enum RpcServiceEndpointError {
    Decode(FrameDecodeError),
    Encode(FrameEncodeError),
    Handler(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for RpcServiceEndpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcServiceEndpointError::Decode(e) => write!(f, "decode error: {e}"),
            RpcServiceEndpointError::Encode(e) => write!(f, "encode error: {e}"),
            RpcServiceEndpointError::Handler(e) => write!(f, "handler error: {e}"),
        }
    }
}

impl std::error::Error for RpcServiceEndpointError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RpcServiceEndpointError::Decode(e) => Some(e),
            RpcServiceEndpointError::Encode(e) => Some(e),
            RpcServiceEndpointError::Handler(e) => e.source(),
        }
    }
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

impl From<io::Error> for RpcServiceEndpointHandlerError {
    fn from(err: io::Error) -> Self {
        let payload = RpcServiceErrorPayload {
            code: RpcServiceErrorCode::Fail, // Default to a 'Fail' code
            message: err.to_string(),
        };
        RpcServiceEndpointHandlerError(payload)
    }
}
