use crate::{
    frame::{FrameDecodeError, FrameEncodeError, FrameKind, FrameMuxStreamDecoder},
    rpc::{RpcHeader, RpcStreamDecoder, RpcStreamEncoder, RpcStreamEvent},
};
use std::collections::HashMap;

pub struct RpcMuxSession {
    next_stream_id: u32,                             // Counter for the next stream ID
    frame_mux_stream_decoder: FrameMuxStreamDecoder, // Decoder that processes frames
    rpc_stream_decoders: HashMap<u32, RpcStreamDecoder>, // Maps stream ID to decoders for individual streams
}

impl RpcMuxSession {
    pub fn new() -> Self {
        Self {
            next_stream_id: 1,
            frame_mux_stream_decoder: FrameMuxStreamDecoder::new(),
            rpc_stream_decoders: HashMap::new(),
        }
    }

    pub fn start_rpc_stream<F>(
        &mut self,
        header: RpcHeader,
        max_chunk_size: usize,
        on_emit: F,
    ) -> Result<RpcStreamEncoder<F>, FrameEncodeError>
    where
        F: FnMut(&[u8]),
    {
        let stream_id = self.next_stream_id;
        self.next_stream_id += 1;

        let rpc_stream_encoder =
            RpcStreamEncoder::new(stream_id, max_chunk_size, &header, on_emit)?;
        Ok(rpc_stream_encoder)
    }

    /// Receives incoming bytes, decodes them, and invokes the provided callback for each event.
    pub fn receive_bytes<F>(
        &mut self,
        input: &[u8],
        mut on_rpc_stream_event: F,
    ) -> Result<(), FrameDecodeError>
    where
        F: FnMut(RpcStreamEvent),
    {
        let frames = self.frame_mux_stream_decoder.pull_bytes(input);

        for frame_result in frames {
            match frame_result {
                Ok(frame) => {
                    let stream_id = frame.inner.stream_id;

                    let rpc_stream_decoder = self
                        .rpc_stream_decoders
                        .entry(stream_id)
                        .or_insert_with(RpcStreamDecoder::new);

                    match rpc_stream_decoder.decode_frame(&frame) {
                        Ok(events) => {
                            for event in events {
                                if matches!(event, RpcStreamEvent::End { .. }) {
                                    self.rpc_stream_decoders.remove(&stream_id);
                                }
                                on_rpc_stream_event(event);
                            }
                        }
                        Err(e) => {
                            // Clean up stream if error
                            self.rpc_stream_decoders.remove(&stream_id);

                            let error_event = RpcStreamEvent::Error {
                                rpc_header_id: None,
                                frame_decode_error: e.clone(),
                            };

                            on_rpc_stream_event(error_event);

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
                        rpc_header_id: None,
                        frame_decode_error: e.clone(),
                    };

                    on_rpc_stream_event(error_event);
                }
            }
        }

        Ok(())
    }
}
