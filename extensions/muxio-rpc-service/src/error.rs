use bitcode::{Decode, Encode};
use std::fmt;
use std::io;

/// The structured, minimal error payload sent over the wire.
#[derive(Debug, Clone, Encode, Decode)]
pub struct RpcServiceErrorPayload {
    pub code: RpcServiceErrorCode,
    pub message: String,
}

/// The three possible failure categories on the server side.
#[derive(Debug, Clone, Copy, Encode, Decode, PartialEq, Eq)]
pub enum RpcServiceErrorCode {
    Fail,     // User-level failure (e.g. invalid request)
    System,   // Crash, panic, or unexpected bug
    NotFound, // No handler registered for method_id
}

/// The complete error type from the RPC caller's perspective.
#[derive(Debug)]
pub enum RpcServiceError {
    /// Transport-level or protocol-level error.
    Transport(io::Error),

    /// Server responded with a structured application/system error.
    Rpc(RpcServiceErrorPayload),

    /// RPC was cancelled or interrupted locally.
    Aborted,
}

impl fmt::Display for RpcServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcServiceError::Transport(e) => write!(f, "Transport error: {e}"),
            RpcServiceError::Rpc(payload) => {
                write!(f, "[{:?}] {}", payload.code, payload.message)
            }
            RpcServiceError::Aborted => write!(f, "RPC was aborted"),
        }
    }
}

impl std::error::Error for RpcServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RpcServiceError::Transport(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for RpcServiceError {
    fn from(e: io::Error) -> Self {
        RpcServiceError::Transport(e)
    }
}

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn transport_display_and_source() {
        let err = RpcServiceError::Transport(io::Error::new(io::ErrorKind::BrokenPipe, "boom"));
        assert_eq!(format!("{err}"), "Transport error: boom");
        let src = std::error::Error::source(&err).expect("transport has source");
        let io_err = src
            .downcast_ref::<io::Error>()
            .expect("source is io::Error");
        assert_eq!(io_err.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(io_err.to_string(), "boom");
    }

    #[test]
    fn rpc_payload_display_has_code_and_message() {
        let err = RpcServiceError::Rpc(RpcServiceErrorPayload {
            code: RpcServiceErrorCode::NotFound,
            message: "no such method".to_string(),
        });
        assert_eq!(format!("{err}"), "[NotFound] no such method");
        assert!(std::error::Error::source(&err).is_none());
    }

    #[test]
    fn aborted_display_and_no_source() {
        let err = RpcServiceError::Aborted;
        assert_eq!(format!("{err}"), "RPC was aborted");
        assert!(std::error::Error::source(&err).is_none());
    }

    #[test]
    fn from_io_error_yields_transport() {
        let err = RpcServiceError::from(io::Error::new(io::ErrorKind::TimedOut, "slow"));
        assert!(matches!(err, RpcServiceError::Transport(_)));
        assert_eq!(format!("{err}"), "Transport error: slow");
    }
}
