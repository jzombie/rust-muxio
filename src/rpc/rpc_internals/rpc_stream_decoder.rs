use crate::{
    constants::{
        RPC_FRAME_FRAME_HEADER_SIZE, RPC_FRAME_ID_OFFSET, RPC_FRAME_METADATA_LENGTH_OFFSET,
        RPC_FRAME_METADATA_LENGTH_SIZE, RPC_FRAME_METHOD_ID_OFFSET, RPC_FRAME_MSG_TYPE_OFFSET,
    },
    frame::{DecodedFrame, FrameDecodeError, FrameKind},
    rpc::rpc_internals::{RpcHeader, RpcMessageType, RpcStreamEvent},
};

pub struct RpcStreamDecoder {
    state: RpcDecoderState,
    header: Option<RpcHeader>,
    rpc_request_id: Option<u32>,
    rpc_method_id: Option<u64>,
    buffer: Vec<u8>,
    meta_len: usize,
}

pub enum RpcDecoderState {
    AwaitHeader,
    AwaitPayload,
    Done,
}

impl RpcStreamDecoder {
    pub fn new() -> Self {
        Self {
            state: RpcDecoderState::AwaitHeader,
            header: None,
            rpc_request_id: None,
            rpc_method_id: None,
            buffer: Vec::new(),
            meta_len: 0,
        }
    }

    pub fn rpc_request_id(&self) -> Option<u32> {
        self.rpc_request_id
    }

    pub fn rpc_method_id(&self) -> Option<u64> {
        self.rpc_method_id
    }

    // Decoding the frame with fixed metadata length
    pub fn decode_rpc_frame(
        &mut self,
        frame: &DecodedFrame,
    ) -> Result<Vec<RpcStreamEvent>, FrameDecodeError> {
        let mut events = Vec::new();

        match self.state {
            RpcDecoderState::AwaitHeader => {
                self.buffer.extend(&frame.inner.payload);

                // If we don't have enough data for the header, just return (we need more data)
                if self.buffer.len() < RPC_FRAME_FRAME_HEADER_SIZE {
                    return Ok(events);
                }

                let rpc_msg_type =
                    match RpcMessageType::try_from(self.buffer[RPC_FRAME_MSG_TYPE_OFFSET]) {
                        Ok(t) => t,
                        Err(_) => return Err(FrameDecodeError::CorruptFrame), // Frame type is invalid
                    };

                let rpc_request_id = u32::from_le_bytes(
                    self.buffer[RPC_FRAME_ID_OFFSET..RPC_FRAME_METHOD_ID_OFFSET]
                        .try_into()
                        .map_err(|_| FrameDecodeError::CorruptFrame)?,
                );

                let method_id = u64::from_le_bytes(
                    self.buffer[RPC_FRAME_METHOD_ID_OFFSET..RPC_FRAME_METADATA_LENGTH_OFFSET]
                        .try_into()
                        .map_err(|_| FrameDecodeError::CorruptFrame)?,
                );
                self.rpc_method_id = Some(method_id);

                // Read the metadata length and check if we have enough data
                let meta_len = u16::from_le_bytes(
                    self.buffer[RPC_FRAME_METADATA_LENGTH_OFFSET
                        ..RPC_FRAME_METADATA_LENGTH_OFFSET + RPC_FRAME_METADATA_LENGTH_SIZE]
                        .try_into()
                        .map_err(|_| FrameDecodeError::CorruptFrame)?,
                ) as usize;

                if self.buffer.len()
                    < RPC_FRAME_METADATA_LENGTH_OFFSET + RPC_FRAME_METADATA_LENGTH_SIZE + meta_len
                {
                    self.meta_len = meta_len;
                    return Ok(events); // Not enough data to decode the full frame yet
                }

                // Now we can safely extract metadata
                let metadata_bytes = self.buffer[RPC_FRAME_METADATA_LENGTH_OFFSET
                    + RPC_FRAME_METADATA_LENGTH_SIZE
                    ..RPC_FRAME_METADATA_LENGTH_OFFSET + RPC_FRAME_METADATA_LENGTH_SIZE + meta_len]
                    .to_vec();

                self.header = Some(RpcHeader {
                    rpc_msg_type,
                    rpc_request_id,
                    method_id,
                    metadata_bytes,
                });

                // Transition state to AwaitPayload after processing header
                self.state = RpcDecoderState::AwaitPayload;

                // Clean the buffer by removing the header and metadata portion
                self.buffer.drain(
                    ..RPC_FRAME_METADATA_LENGTH_OFFSET + RPC_FRAME_METADATA_LENGTH_SIZE + meta_len,
                );

                let rpc_header = self.header.clone().ok_or(FrameDecodeError::CorruptFrame)?;
                self.rpc_request_id = Some(rpc_header.rpc_request_id);

                // Push the header event
                events.push(RpcStreamEvent::Header {
                    rpc_request_id,
                    rpc_method_id: method_id,
                    rpc_header,
                });

                // Continue processing payload if available
                if !self.buffer.is_empty() {
                    events.push(RpcStreamEvent::PayloadChunk {
                        rpc_request_id,
                        rpc_method_id: method_id,
                        bytes: self.buffer.split_off(0),
                    });
                }
            }
            RpcDecoderState::AwaitPayload => {
                // If we encounter the end of the stream
                if frame.inner.kind == FrameKind::End {
                    self.state = RpcDecoderState::Done;
                    events.push(RpcStreamEvent::End {
                        rpc_request_id: self
                            .rpc_request_id
                            .ok_or(FrameDecodeError::CorruptFrame)?,
                        rpc_method_id: self.rpc_method_id.ok_or(FrameDecodeError::CorruptFrame)?,
                    });
                } else if frame.inner.kind == FrameKind::Cancel {
                    return Err(FrameDecodeError::ReadAfterCancel); // Stop processing further frames
                } else {
                    // If there's a payload chunk, append it to the events
                    events.push(RpcStreamEvent::PayloadChunk {
                        rpc_request_id: self
                            .rpc_request_id
                            .ok_or(FrameDecodeError::CorruptFrame)?,
                        rpc_method_id: self.rpc_method_id.ok_or(FrameDecodeError::CorruptFrame)?,
                        bytes: frame.inner.payload.clone(),
                    });
                }
            }
            RpcDecoderState::Done => {
                // If the stream is done, we should not process further frames
            }
        }

        Ok(events)
    }
}
