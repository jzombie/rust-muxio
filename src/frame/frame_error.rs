use std::fmt;

#[derive(Debug, PartialEq)]
pub enum FrameEncodeError {
    CorruptFrame,

    /// Attempted to write to a stream that has already ended.
    WriteAfterEnd,

    /// Attempted to write to a stream that was canceled prematurely.
    WriteAfterCancel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FrameDecodeError {
    CorruptFrame,

    /// Attempted to write to a stream that has already ended.
    ReadAfterEnd,

    /// Attempted to write to a stream that was canceled prematurely.
    ReadAfterCancel,

    IncompleteHeader,
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
        }
    }
}

impl std::error::Error for FrameDecodeError {}
