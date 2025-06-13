use std::fmt;
use std::io;

/// Represents errors that can occur during an RPC call from the perspective of the caller.
#[derive(Debug)]
pub enum RpcCallerError {
    /// A transport-level or I/O error occurred during the call.
    Io(io::Error),
    /// The remote handler executed but explicitly returned an application-level error.
    /// The payload contains the custom error data sent by the server.
    RemoteError { payload: Vec<u8> },
    /// The remote endpoint indicated a system-level failure (e.g., method not found, server panic).
    RemoteSystemError(String),
    /// The operation was aborted before a result could be determined.
    Aborted,
}

impl fmt::Display for RpcCallerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcCallerError::Io(e) => write!(f, "I/O error: {}", e),
            RpcCallerError::RemoteError { payload } => {
                write!(f, "Remote handler failed with payload: {:?}", payload)
            }
            RpcCallerError::RemoteSystemError(msg) => write!(f, "Remote system error: {}", msg),
            RpcCallerError::Aborted => write!(f, "RPC call aborted"),
        }
    }
}

impl std::error::Error for RpcCallerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RpcCallerError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for RpcCallerError {
    fn from(e: io::Error) -> Self {
        RpcCallerError::Io(e)
    }
}
