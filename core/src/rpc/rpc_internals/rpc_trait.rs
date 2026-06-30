use super::RpcStreamEvent;
use crate::frame::FrameDecodeError;
use std::sync::Arc;

/// Type alias for a response writer closure that sends framed chunks.
///
/// The closure receives `(chunk_bytes, is_finalized)`. When `is_finalized` is
/// `true`, the stream is ended after writing the chunk.
pub type RpcResponseWriter = Box<dyn FnMut(&[u8], bool) + Send>;

/// Thread-safe buffer for queuing response chunks before a writer is available.
///
/// Each entry is `(chunk_bytes, is_finalized)`. Used by `StreamResponder` to
/// buffer writes that occur before `read_bytes` installs the real encoder-backed writer.
pub type RpcResponseBuffer = Arc<std::sync::Mutex<Vec<(Vec<u8>, bool)>>>;

/// Type alias for a streaming method router closure.
///
/// Maps `(method_id, request_id)` to an optional per-request response handler.
/// When a handler is returned, it is registered for that stream and subsequent
/// events bypass the catch-all accumulator.
pub type RpcStreamMethodRouter<'a> =
    Box<dyn FnMut(u64, u32) -> Option<Box<dyn FnMut(RpcStreamEvent) + Send + 'a>> + Send + 'a>;

/// Trait alias for any mutable function or closure that emits a byte slice.
///
/// This trait is used by encoders or stream managers that need to send raw
/// byte payloads over an output channel (e.g., a socket, buffer, or transport).
///
/// The function should emit a complete and valid frame fragment.
/// Typically used as a sink.
pub trait RpcEmit: FnMut(&[u8]) {}
impl<T: FnMut(&[u8])> RpcEmit for T {}

/// Trait alias for response handlers that consume [`RpcStreamEvent`]`s.
///
/// Implementors must be `Send` to allow handling in concurrent or async contexts.
/// This is used for registering per-request response callbacks in a client session.
///
/// Each event corresponds to part of an RPC response stream, such as [`RpcStreamEvent::Header`],
/// [`RpcStreamEvent::PayloadChunk`], or [`RpcStreamEvent::End`].
pub trait RpcResponseHandler: FnMut(RpcStreamEvent) + Send {}
impl<T: FnMut(RpcStreamEvent) + Send> RpcResponseHandler for T {}

/// Trait alias for handlers that decode and process [`RpcStreamEvent`]s fallibly.
///
/// This trait allows the handler to return a `Result` to signal whether
/// the event was processed successfully or should terminate processing
/// due to an error.
///
/// Used when decoding and interpreting streamed RPC messages.
///
/// Example:
/// ```ignore
/// let decoder = |event: RpcStreamEvent| -> Result<(), FrameDecodeError> {
///     match event {
///         RpcStreamEvent::Header { .. } => Ok(()),
///         _ => Err(FrameDecodeError::CorruptFrame),
///     }
/// };
/// ```
pub trait RpcStreamEventDecoderHandler:
    FnMut(RpcStreamEvent) -> Result<(), FrameDecodeError>
{
}
impl<T: FnMut(RpcStreamEvent) -> Result<(), FrameDecodeError>> RpcStreamEventDecoderHandler for T {}
