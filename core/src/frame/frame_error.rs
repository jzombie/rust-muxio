use std::fmt;

#[derive(Debug, PartialEq)]
pub enum FrameEncodeError {
    CorruptFrame,

    /// Attempted to write to a stream that has already ended.
    WriteAfterEnd,

    /// Attempted to write to a stream that was canceled prematurely.
    WriteAfterCancel,
}

impl fmt::Display for FrameEncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameEncodeError::CorruptFrame => write!(f, "Corrupt frame"),
            FrameEncodeError::WriteAfterEnd => write!(f, "Write after stream ended"),
            FrameEncodeError::WriteAfterCancel => write!(f, "Write after stream cancelled"),
        }
    }
}

impl std::error::Error for FrameEncodeError {}

#[derive(Debug, Clone, PartialEq)]
pub enum FrameDecodeError {
    CorruptFrame,

    /// Attempted to write to a stream that has already ended.
    ReadAfterEnd,

    /// Attempted to write to a stream that was canceled prematurely.
    ReadAfterCancel,

    IncompleteHeader,

    /// Transport-level I/O error carrying the real `std::io::Error` display.
    /// Used by `fail_all_pending_requests` to surface the root cause
    /// (e.g. `ConnectionReset`, `BrokenPipe`, `UnexpectedEof`) instead of a
    /// static `ReadAfterCancel`.
    Transport(String),
}

impl fmt::Display for FrameDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameDecodeError::CorruptFrame => write!(f, "Corrupt frame detected"),
            FrameDecodeError::ReadAfterEnd => {
                write!(f, "Attempted to read from a stream that has already ended")
            }
            FrameDecodeError::ReadAfterCancel => {
                write!(f, "Attempted to read from a cancelled stream")
            }
            FrameDecodeError::IncompleteHeader => write!(f, "Incomplete frame header received"),
            FrameDecodeError::Transport(msg) => write!(f, "Transport error: {msg}"),
        }
    }
}

impl std::error::Error for FrameDecodeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_frame_encode_errors() {
        assert_eq!(FrameEncodeError::CorruptFrame.to_string(), "Corrupt frame");
        assert_eq!(
            FrameEncodeError::WriteAfterEnd.to_string(),
            "Write after stream ended"
        );
        assert_eq!(
            FrameEncodeError::WriteAfterCancel.to_string(),
            "Write after stream cancelled"
        );
        // Error trait impl is just Display
        let _: &dyn std::error::Error = &FrameEncodeError::CorruptFrame;
    }

    #[test]
    fn display_frame_decode_errors() {
        assert_eq!(
            FrameDecodeError::CorruptFrame.to_string(),
            "Corrupt frame detected"
        );
        assert_eq!(
            FrameDecodeError::ReadAfterEnd.to_string(),
            "Attempted to read from a stream that has already ended"
        );
        assert_eq!(
            FrameDecodeError::ReadAfterCancel.to_string(),
            "Attempted to read from a cancelled stream"
        );
        assert_eq!(
            FrameDecodeError::IncompleteHeader.to_string(),
            "Incomplete frame header received"
        );
        let _: &dyn std::error::Error = &FrameDecodeError::CorruptFrame;
    }

    #[test]
    fn display_frame_decode_transport_error() {
        let err = FrameDecodeError::Transport("ConnectionReset".to_string());
        assert_eq!(err.to_string(), "Transport error: ConnectionReset");
        let _: &dyn std::error::Error = &err;

        let empty = FrameDecodeError::Transport(String::new());
        assert_eq!(empty.to_string(), "Transport error: ");
        let _: &dyn std::error::Error = &empty;

        let eof = FrameDecodeError::Transport("unexpected EOF (connection closed)".to_string());
        assert!(eof.to_string().contains("unexpected EOF"));
        let _: &dyn std::error::Error = &eof;

        assert_eq!(
            FrameDecodeError::Transport("a".to_string()),
            FrameDecodeError::Transport("a".to_string())
        );
        assert_ne!(
            FrameDecodeError::Transport("a".to_string()),
            FrameDecodeError::CorruptFrame
        );
    }
}
