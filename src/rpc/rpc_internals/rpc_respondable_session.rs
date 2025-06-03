use crate::frame::{FrameDecodeError, FrameEncodeError};
use crate::rpc::rpc_internals::{RpcHeader, RpcSession, RpcStreamEncoder, RpcStreamEvent};
use std::collections::HashMap;

/// Lightweight wrapper over `RpcSession` that tracks response handlers.
///
/// This struct allows the caller to associate a response callback per
/// outgoing request. It also supports an optional global fallback handler
/// for unmatched or unsolicited events.
///
/// Suitable for simple scenarios where dispatch logic is externally managed.
pub struct RpcRespondableSession<'a> {
    rpc_session: RpcSession,
    // TODO: Make these names less vague
    response_handlers: HashMap<u32, Box<dyn FnMut(RpcStreamEvent) + Send + 'a>>,
    catch_all_response_handler: Option<Box<dyn FnMut(RpcStreamEvent) + Send + 'a>>,
    pre_buffered_responses: HashMap<u32, Vec<u8>>, // Track buffered responses by request ID
    pre_buffering_flags: HashMap<u32, bool>, // Track whether pre-buffering is enabled for each request
}

impl<'a> RpcRespondableSession<'a> {
    pub fn new() -> Self {
        Self {
            rpc_session: RpcSession::new(),
            response_handlers: HashMap::new(),
            catch_all_response_handler: None,
            pre_buffered_responses: HashMap::new(),
            pre_buffering_flags: HashMap::new(),
        }
    }

    // TODO: Document that prebuffering buffers the entire response payload into a single chunk
    pub fn init_respondable_request<G, F>(
        &mut self,
        hdr: RpcHeader,
        max_chunk_size: usize,
        on_emit: G,
        on_response: Option<F>,
        pre_buffer_response: bool,
    ) -> Result<RpcStreamEncoder<G>, FrameEncodeError>
    where
        G: FnMut(&[u8]),
        F: FnMut(RpcStreamEvent) + Send + 'a,
    {
        let rpc_header_id = hdr.id;

        // Set pre-buffering flag for this specific request
        self.pre_buffering_flags
            .insert(rpc_header_id, pre_buffer_response);

        if let Some(on_response) = on_response {
            self.response_handlers
                .insert(rpc_header_id, Box::new(on_response));
        }

        self.rpc_session
            .init_request(hdr, max_chunk_size, on_emit)
            .map_err(|_| FrameEncodeError::CorruptFrame)
    }

    pub fn start_reply_stream<F>(
        &mut self,
        hdr: RpcHeader,
        max_chunk_size: usize,
        on_emit: F,
    ) -> Result<RpcStreamEncoder<F>, FrameEncodeError>
    where
        F: FnMut(&[u8]),
    {
        self.rpc_session
            .init_request(hdr, max_chunk_size, on_emit)
            .map_err(|_| FrameEncodeError::CorruptFrame)
    }

    // TODO: Document
    // Invoked on the remote in response to `init_respondable_request` from the local client
    pub fn set_catch_all_response_handler<F>(&mut self, handler: F)
    where
        F: FnMut(RpcStreamEvent) + Send + 'a,
    {
        self.catch_all_response_handler = Some(Box::new(handler));
    }

    pub fn receive_bytes(&mut self, bytes: &[u8]) -> Result<(), FrameDecodeError> {
        self.rpc_session.receive_bytes(bytes, |evt| {
            let id = match &evt {
                RpcStreamEvent::Header { rpc_header_id, .. } => Some(*rpc_header_id),
                RpcStreamEvent::PayloadChunk { rpc_header_id, .. } => Some(*rpc_header_id),
                RpcStreamEvent::End { rpc_header_id, .. } => Some(*rpc_header_id),
                RpcStreamEvent::Error { rpc_header_id, .. } => *rpc_header_id,
            };

            let method_id = match &evt {
                RpcStreamEvent::Header { rpc_method_id, .. } => Some(*rpc_method_id),
                RpcStreamEvent::PayloadChunk { rpc_method_id, .. } => Some(*rpc_method_id),
                RpcStreamEvent::End { rpc_method_id, .. } => Some(*rpc_method_id),
                RpcStreamEvent::Error { rpc_method_id, .. } => *rpc_method_id,
            };

            let mut handled = false;

            if let Some(rpc_id) = id {
                let is_pre_buffering_response = match self.pre_buffering_flags.get(&rpc_id) {
                    Some(bool) => bool,
                    None => &false,
                };

                if *is_pre_buffering_response {
                    // Accumulate the bytes into the buffer for this request ID
                    let buffer = self
                        .pre_buffered_responses
                        .entry(rpc_id)
                        .or_insert_with(|| Vec::new());

                    match &evt {
                        RpcStreamEvent::Header { .. } => {
                            if let Some(cb) = self.response_handlers.get_mut(&rpc_id) {
                                cb(evt.clone());
                            }
                        }

                        RpcStreamEvent::PayloadChunk { bytes, .. } => {
                            buffer.extend_from_slice(bytes);
                        }
                        RpcStreamEvent::End { .. } => {
                            // When the end of the stream is reached, call the response handler
                            if let Some(cb) = self.response_handlers.get_mut(&rpc_id) {
                                let rpc_payload_event = RpcStreamEvent::PayloadChunk {
                                    rpc_header_id: rpc_id,
                                    rpc_method_id: method_id.unwrap(), // TODO: Don't use unwrap here
                                    bytes: buffer.clone(),
                                };

                                cb(rpc_payload_event);
                                cb(evt.clone());

                                self.pre_buffered_responses.remove(&rpc_id); // Clear the buffer after calling
                            }
                        }
                        _ => {
                            // TODO: Handle
                        }
                    }
                } else {
                    if let Some(cb) = self.response_handlers.get_mut(&rpc_id) {
                        cb(evt.clone());
                        handled = true;
                    }
                }

                if matches!(
                    evt,
                    RpcStreamEvent::End { .. } | RpcStreamEvent::Error { .. }
                ) {
                    self.response_handlers.remove(&rpc_id);
                }
            }

            if !handled {
                if let Some(cb) = self.catch_all_response_handler.as_mut() {
                    cb(evt);
                }
            }
        })?;

        Ok(())
    }

    pub fn get_remaining_response_handlers(&self) -> usize {
        self.response_handlers.len()
    }
}
