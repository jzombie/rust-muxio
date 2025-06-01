#[derive(Debug, PartialEq)]
pub enum FrameEncodeError {
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
