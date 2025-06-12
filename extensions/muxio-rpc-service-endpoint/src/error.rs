use muxio::frame::{FrameDecodeError, FrameEncodeError};

/// Represents possible errors that can occur within an RPC service endpoint.
///
/// This error type unifies errors from frame decoding, frame encoding,
/// and user-provided handler execution, allowing higher-level methods
/// to propagate a single error type.
///
/// Variants:
///
/// - `Decode`: Indicates an error occurred while parsing an incoming RPC frame.
/// - `Encode`: Indicates an error occurred while serializing an outgoing RPC frame.
/// - `Handler`: Represents any error returned by a user-provided RPC handler,
///    typically during request processing.
#[derive(Debug)]
pub enum RpcServiceEndpointError {
    /// Error that occurred while decoding an inbound frame.
    Decode(FrameDecodeError),

    /// Error that occurred while encoding an outbound frame.
    Encode(FrameEncodeError),

    // TODO: Use type alias
    /// Error returned from a user-defined handler function.
    Handler(Box<dyn std::error::Error + Send + Sync>),
}
