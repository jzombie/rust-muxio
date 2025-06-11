use muxio::frame::{FrameDecodeError, FrameEncodeError};

#[derive(Debug)]
pub enum RpcEndpointError {
    Decode(FrameDecodeError),
    Encode(FrameEncodeError),
    Handler(Box<dyn std::error::Error + Send + Sync>),
}
