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
