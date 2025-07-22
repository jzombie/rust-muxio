use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;

/// The structured, minimal error payload sent over the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcErrorPayload {
    pub code: RpcErrorCode,
    pub message: String,
}

/// The three possible failure categories on the server side.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RpcErrorCode {
    Fail,     // User-level failure (e.g. invalid request)
    System,   // Crash, panic, or unexpected bug
    NotFound, // No handler registered for method_id
}

/// The complete error type from the RPC caller's perspective.
#[derive(Debug)]
pub enum RpcCallerError {
    /// Transport-level or protocol-level error.
    Transport(io::Error),

    /// Server responded with a structured application/system error.
    Rpc(RpcErrorPayload),

    /// RPC was cancelled or interrupted locally.
    Aborted,
}

impl fmt::Display for RpcCallerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcCallerError::Transport(e) => write!(f, "Transport error: {e}"),
            RpcCallerError::Rpc(payload) => {
                write!(f, "[{:?}] {}", payload.code, payload.message)
            }
            RpcCallerError::Aborted => write!(f, "RPC was aborted"),
        }
    }
}

impl std::error::Error for RpcCallerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RpcCallerError::Transport(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for RpcCallerError {
    fn from(e: io::Error) -> Self {
        RpcCallerError::Transport(e)
    }
}
