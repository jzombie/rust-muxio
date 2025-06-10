use crate::constants::{FRAME_HEADER_SIZE, FRAME_LENGTH_FIELD_SIZE};
use crate::frame::{DecodedFrame, FrameCodec, FrameDecodeError, FrameKind};
use std::collections::{BTreeMap, HashMap, VecDeque};

// TODO: Add optional `UDP mode` which ensures frames have been received on remote

/// A multiplexing frame decoder for interleaved stream data.
///
/// `FrameStreamDecoder` accepts a continuous stream of bytes that may contain
/// multiple interleaved logical streams, each identified by a `stream_id`.
///
/// This decoder maintains internal reassembly state for each stream, emitting
/// complete, in-order `Frame`s only when all prior frames for that stream have
/// arrived. It supports out-of-order delivery within each stream and handles
/// `Cancel` and `End` control frames.
///
/// ### Key Characteristics:
/// - **Multiplexed**: Handles multiple `stream_id`s concurrently.
/// - **Stateful**: Buffers incomplete frames per stream.
/// - **Terminating**: Automatically removes streams upon `Cancel` or completed `End`.
///
/// Unlike `FrameStreamEncoder`, which is a single-stream encoder tied to one
/// `stream_id`, this decoder supports all active streams simultaneously.
///
/// ### Behavior Summary:
/// - A `Cancel` frame causes immediate stream removal and emits a
///   `StreamTerminated` error.
/// - An `End` frame marks the stream as complete and triggers stream
///   cleanup once buffered frames have been flushed.
/// - Invalid or malformed frames yield `CorruptFrame`.
pub struct FrameMuxStreamDecoder {
    buffer: Vec<u8>,                         // Holds partial frame data
    streams: HashMap<u32, StreamReassembly>, // Stores reassembled frames
}

struct StreamReassembly {
    next_expected: u32,                  // The next expected sequence ID
    buffer: BTreeMap<u32, DecodedFrame>, // Holds frames that are out-of-order
    is_canceled: bool,
    is_ended: bool,
}

pub struct FrameDecoderIterator {
    queue: VecDeque<Result<DecodedFrame, FrameDecodeError>>,
}

impl Iterator for FrameDecoderIterator {
    type Item = Result<DecodedFrame, FrameDecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.queue.pop_front()
    }
}

impl FrameMuxStreamDecoder {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            streams: HashMap::new(),
        }
    }

    // Reads new bytes and attempts to decode them into frames
    pub fn read_bytes(&mut self, data: &[u8]) -> FrameDecoderIterator {
        self.buffer.extend_from_slice(data);
        let mut queue = VecDeque::new();

        while self.buffer.len() >= FRAME_LENGTH_FIELD_SIZE {
            let len = match self
                .buffer
                .get(..FRAME_LENGTH_FIELD_SIZE)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u32::from_le_bytes)
            {
                Some(n) => n as usize,
                None => {
                    queue.push_back(Err(FrameDecodeError::IncompleteHeader));
                    break;
                }
            };

            let total = FRAME_HEADER_SIZE + len;

            if self.buffer.len() < total {
                break;
            }

            match FrameCodec::decode(&self.buffer[..total]) {
                Ok(mut frame) => {
                    let stream_id = frame.inner.stream_id;
                    let frame_kind = frame.inner.kind;

                    self.buffer.drain(..total);

                    if let Some(stream) = self.streams.get(&stream_id) {
                        if stream.is_canceled {
                            frame.decode_error = Some(FrameDecodeError::ReadAfterCancel);
                            queue.push_back(Ok(frame));
                            continue;
                        }

                        // Note: We do not check `stream.is_ended` here because frames may arrive out of order.
                        // The `End` frame could be received before all prior data frames. In contrast,
                        // a canceled stream is always considered terminated immediately and must be discarded.
                    }

                    if frame_kind == FrameKind::Cancel {
                        if let Some(stream) = self.streams.get_mut(&stream_id) {
                            stream.is_canceled = true;
                        }

                        frame.decode_error = Some(FrameDecodeError::ReadAfterCancel);
                        queue.push_back(Ok(frame));
                        self.streams.remove(&stream_id);
                        continue;
                    }

                    let stream =
                        self.streams
                            .entry(stream_id)
                            .or_insert_with(|| StreamReassembly {
                                next_expected: 0,
                                buffer: BTreeMap::new(),
                                is_canceled: false,
                                is_ended: false,
                            });

                    if frame_kind == FrameKind::End {
                        stream.is_ended = true;
                    }

                    stream.buffer.insert(frame.inner.seq_id, frame);

                    while let Some(buffered_frame) = stream.buffer.remove(&stream.next_expected) {
                        stream.next_expected += 1;
                        queue.push_back(Ok(buffered_frame));
                    }

                    if stream.is_ended && stream.buffer.is_empty() {
                        self.streams.remove(&stream_id);
                    }
                }
                Err(e) => {
                    self.buffer.drain(..total);
                    queue.push_back(Err(e));
                    continue;
                }
            }
        }

        FrameDecoderIterator { queue }
    }
}
