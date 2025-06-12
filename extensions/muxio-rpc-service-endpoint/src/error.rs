use muxio::frame::{FrameDecodeError, FrameEncodeError};

#[derive(Debug)]
pub enum RpcServiceEndpointError {
    Decode(FrameDecodeError),
    Encode(FrameEncodeError),
    Handler(Box<dyn std::error::Error + Send + Sync>),
}

// Add this From impl to allow `?` to convert FrameDecodeError
impl From<FrameDecodeError> for RpcServiceEndpointError {
    fn from(err: FrameDecodeError) -> Self {
        RpcServiceEndpointError::Decode(err)
    }
}

// Add this From impl to allow `?` to convert FrameEncodeError
impl From<FrameEncodeError> for RpcServiceEndpointError {
    fn from(err: FrameEncodeError) -> Self {
        RpcServiceEndpointError::Encode(err)
    }
}
