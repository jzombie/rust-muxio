use super::rpc_trait::*;
use crate::{
    frame::{FrameDecodeError, FrameEncodeError, FrameKind, FrameMuxStreamDecoder},
    rpc::rpc_internals::{RpcHeader, RpcStreamDecoder, RpcStreamEncoder, RpcStreamEvent},
    utils::increment_u32_id,
};
use std::collections::HashMap;

impl Default for RpcSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Low-level stream multiplexing engine for RPC.
///
/// This struct manages the allocation of stream IDs, the decoding of framed
/// messages, and per-stream decoding state. It does not perform any routing
/// or application-le
pub struct RpcSession {
    next_stream_id: u32,                             // Counter for the next stream ID
    frame_mux_stream_decoder: FrameMuxStreamDecoder, // Decoder that processes frames
    rpc_stream_decoders: HashMap<u32, RpcStreamDecoder>, // Maps stream ID to decoders for individual streams
}

impl RpcSession {
    pub fn new() -> Self {
        Self {
            next_stream_id: increment_u32_id(),
            frame_mux_stream_decoder: FrameMuxStreamDecoder::new(),
            rpc_stream_decoders: HashMap::new(),
        }
    }

    pub fn init_request<E>(
        &mut self,
        header: RpcHeader,
        max_chunk_size: usize,
        on_emit: E,
    ) -> Result<RpcStreamEncoder<E>, FrameEncodeError>
    where
        E: RpcEmit,
    {
        let stream_id = self.next_stream_id;
        self.next_stream_id = increment_u32_id();

        let rpc_stream_encoder =
            RpcStreamEncoder::new(stream_id, max_chunk_size, &header, on_emit)?;
        Ok(rpc_stream_encoder)
    }

    /// Receives incoming bytes, decodes them, and invokes the provided callback for each event.
    pub fn read_bytes<H>(
        &mut self,
        input: &[u8],
        mut on_rpc_stream_event: H,
    ) -> Result<(), FrameDecodeError>
    where
        H: RpcStreamEventDecoderHandler,
    {
        let frames = self.frame_mux_stream_decoder.read_bytes(input);

        for frame_result in frames {
            match frame_result {
                Ok(frame) => {
                    let stream_id = frame.inner.stream_id;

                    let rpc_stream_decoder = self.rpc_stream_decoders.entry(stream_id).or_default();

                    match rpc_stream_decoder.decode_rpc_frame(&frame) {
                        Ok(events) => {
                            for event in events {
                                if matches!(event, RpcStreamEvent::End { .. }) {
                                    self.rpc_stream_decoders.remove(&stream_id);
                                }

                                on_rpc_stream_event(event)?;
                            }
                        }
                        Err(e) => {
                            // Clean up stream if error
                            self.rpc_stream_decoders.remove(&stream_id);

                            let error_event = RpcStreamEvent::Error {
                                rpc_header: None,
                                rpc_request_id: None,
                                rpc_method_id: None,
                                frame_decode_error: e.clone(),
                            };

                            on_rpc_stream_event(error_event)?;

                            return Err(e);
                        }
                    }

                    // Ensure cleanup of old frames
                    if frame.inner.kind == FrameKind::Cancel || frame.inner.kind == FrameKind::End {
                        self.rpc_stream_decoders.remove(&stream_id);
                    }
                }
                Err(e) => {
                    let error_event = RpcStreamEvent::Error {
                        rpc_header: None,
                        rpc_request_id: None,
                        rpc_method_id: None,
                        frame_decode_error: e.clone(),
                    };

                    on_rpc_stream_event(error_event)?;
                }
            }
        }

        Ok(())
    }
}
