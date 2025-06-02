use crate::frame::{Frame, FrameCodec, FrameEncodeError, FrameKind};
use crate::utils::now;

// TODO: Add optional `UDP mode` which ensures frames have been received on remote

/// Encodes a stream of bytes into framed messages.
///
/// Each frame is emitted with proper metadata. Frames are emitted when enough
/// data is buffered or when explicitly flushed. Optionally, a callback can be
/// registered to observe each encoded frame.
pub struct FrameStreamEncoder<F>
where
    F: FnMut(&[u8]),
{
    stream_id: u32,
    max_chunk_size: usize,
    next_seq_id: u32,
    next_kind: FrameKind,
    buffer: Vec<u8>,
    is_canceled: bool,
    is_ended: bool,
    on_emit: F,
}

impl<F> FrameStreamEncoder<F>
where
    F: FnMut(&[u8]),
{
    // TODO: Result error if already used stream id
    /// Creates a new encoder with the given stream ID and payload limit.
    ///
    /// An optional `on_emit` callback will be invoked with every emitted
    /// frame (as raw encoded bytes).
    pub fn new(stream_id: u32, max_chunk_size: usize, on_emit: F) -> Self {
        Self {
            stream_id,
            max_chunk_size,
            next_seq_id: 0,
            next_kind: FrameKind::Open,
            buffer: Vec::new(),
            is_canceled: false,
            is_ended: false,
            on_emit,
        }
    }

    fn emit_frame(&mut self, frame: Frame) -> Result<usize, FrameEncodeError> {
        if self.is_canceled {
            return Err(FrameEncodeError::WriteAfterCancel);
        } else if self.is_ended {
            return Err(FrameEncodeError::WriteAfterEnd);
        }

        let bytes = FrameCodec::encode(&frame);

        (self.on_emit)(&bytes);

        Ok(bytes.len())
    }

    /// Accepts some bytes, returns zero or more encoded frames (ready to send).
    /// Buffers any leftover partial chunk internally.
    pub fn push_bytes(&mut self, data: &[u8]) -> Result<usize, FrameEncodeError> {
        if self.is_canceled {
            return Err(FrameEncodeError::WriteAfterCancel);
        } else if self.is_ended {
            return Err(FrameEncodeError::WriteAfterEnd);
        }

        self.buffer.extend_from_slice(data);
        let mut op_written_bytes: usize = 0;

        while self.buffer.len() >= self.max_chunk_size {
            let chunk = self.buffer.drain(..self.max_chunk_size).collect::<Vec<_>>();

            let frame = Frame {
                stream_id: self.stream_id,
                seq_id: self.next_seq_id,
                kind: self.next_kind,
                timestamp_micros: now(),
                payload: chunk,
            };

            op_written_bytes += self.emit_frame(frame)?;

            self.next_seq_id += 1;
            self.next_kind = FrameKind::Data;
        }

        Ok(op_written_bytes)
    }

    /// Flushes remaining buffered data as a final frame (could be partial).
    pub fn flush(&mut self) -> Result<usize, FrameEncodeError> {
        if self.is_canceled {
            return Err(FrameEncodeError::WriteAfterCancel);
        } else if self.is_ended {
            return Err(FrameEncodeError::WriteAfterEnd);
        }

        if self.buffer.is_empty() {
            return Ok(0);
        }

        let frame = Frame {
            stream_id: self.stream_id,
            seq_id: self.next_seq_id,
            kind: self.next_kind,
            timestamp_micros: now(),
            payload: self.buffer.split_off(0),
        };

        let op_written_bytes = self.emit_frame(frame)?;

        self.next_seq_id += 1;
        self.next_kind = FrameKind::Data;

        Ok(op_written_bytes)
    }

    /// Returns an End frame, even if no data was ever sent.
    pub fn end_stream(&mut self) -> Result<usize, FrameEncodeError> {
        if self.is_canceled {
            return Err(FrameEncodeError::WriteAfterCancel);
        } else if self.is_ended {
            return Err(FrameEncodeError::WriteAfterEnd);
        }

        let frame = Frame {
            stream_id: self.stream_id,
            seq_id: self.next_seq_id,
            kind: FrameKind::End,
            timestamp_micros: now(),
            payload: self.buffer.split_off(0),
        };

        let op_written_bytes = self.emit_frame(frame)?;

        self.is_ended = true;

        Ok(op_written_bytes)
    }

    /// Emits a Cancel frame for this stream.
    pub fn cancel_stream(&mut self) -> Result<usize, FrameEncodeError> {
        if self.is_canceled {
            return Err(FrameEncodeError::WriteAfterCancel);
        } else if self.is_ended {
            return Err(FrameEncodeError::WriteAfterEnd);
        }

        let frame = Frame {
            stream_id: self.stream_id,
            seq_id: self.next_seq_id,
            kind: FrameKind::Cancel,
            timestamp_micros: now(),
            payload: Vec::new(),
        };

        let op_written_bytes = self.emit_frame(frame)?;

        self.is_canceled = true;

        Ok(op_written_bytes)
    }
}
