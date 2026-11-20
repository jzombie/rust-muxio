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

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn handler_error_display_and_source() {
        let err = RpcServiceEndpointHandlerError(RpcServiceErrorPayload {
            code: RpcServiceErrorCode::Fail,
            message: "bad input".to_string(),
        });
        assert_eq!(format!("{err}"), "Handler failed with code Fail: bad input");
        assert!(std::error::Error::source(&err).is_none());
    }

    #[test]
    fn handler_error_from_io_error_defaults_to_fail() {
        let err = RpcServiceEndpointHandlerError::from(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nope",
        ));
        assert_eq!(err.0.code, RpcServiceErrorCode::Fail);
        assert_eq!(err.0.message, "nope");
    }

    #[test]
    fn endpoint_error_display_covers_all_variants() {
        use std::fmt;

        #[derive(Debug)]
        struct Leaf;
        impl fmt::Display for Leaf {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "leaf")
            }
        }
        impl std::error::Error for Leaf {}

        #[derive(Debug)]
        struct Mid;
        impl fmt::Display for Mid {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "mid")
            }
        }
        impl std::error::Error for Mid {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                static LEAF: Leaf = Leaf;
                Some(&LEAF)
            }
        }

        let decode = RpcServiceEndpointError::Decode(FrameDecodeError::IncompleteHeader);
        assert_eq!(
            format!("{decode}"),
            "decode error: Incomplete frame header received"
        );
        assert!(std::error::Error::source(&decode).is_some());

        let encode = RpcServiceEndpointError::Encode(FrameEncodeError::WriteAfterEnd);
        assert_eq!(
            format!("{encode}"),
            "encode error: Write after stream ended"
        );
        assert!(std::error::Error::source(&encode).is_some());

        // The boxed handler error must propagate its inner source, provable
        // by downcasting back to the concrete leaf type.
        let handler = RpcServiceEndpointError::Handler(Box::new(Mid));
        assert_eq!(format!("{handler}"), "handler error: mid");
        let src = std::error::Error::source(&handler).expect("handler has source");
        assert!(
            src.downcast_ref::<Leaf>().is_some(),
            "expected Leaf source, got {src:?}"
        );
    }

    #[test]
    fn endpoint_error_from_impls() {
        let decode: RpcServiceEndpointError = FrameDecodeError::IncompleteHeader.into();
        assert!(matches!(decode, RpcServiceEndpointError::Decode(_)));
        let encode: RpcServiceEndpointError = FrameEncodeError::WriteAfterEnd.into();
        assert!(matches!(encode, RpcServiceEndpointError::Encode(_)));
    }
}
