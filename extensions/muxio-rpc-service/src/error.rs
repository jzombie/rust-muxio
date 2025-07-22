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
