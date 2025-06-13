use muxio::frame::{FrameDecodeError, FrameEncodeError};
use std::fmt;

/// A special error type that wraps a byte payload for the client.
///
/// When a handler returns this specific error, the endpoint will send its
/// contents back to the client with a `Fail` status. Any other error type
/// will result in a generic `SystemError`.
#[derive(Debug)]
pub struct HandlerPayloadError(pub Vec<u8>);

impl fmt::Display for HandlerPayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handler failed with a custom payload for the client")
    }
}

impl std::error::Error for HandlerPayloadError {}

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
