use crate::frame::{FrameDecodeError, FrameEncodeError};
use crate::rpc::rpc_internals::{
    RpcHeader, RpcSession, RpcStreamEncoder, RpcStreamEvent,
    rpc_trait::{RpcEmit, RpcResponseHandler},
};
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
    prebuffered_responses: HashMap<u32, Vec<u8>>, // Track buffered responses by request ID
    prebuffering_flags: HashMap<u32, bool>, // Track whether pre-buffering is enabled for each request
}

impl<'a> RpcRespondableSession<'a> {
    pub fn new() -> Self {
        Self {
            rpc_session: RpcSession::new(),
            response_handlers: HashMap::new(),
            catch_all_response_handler: None,
            prebuffered_responses: HashMap::new(),
            prebuffering_flags: HashMap::new(),
        }
    }

    // TODO: Document that prebuffering buffers the entire response payload into a single chunk
    pub fn init_respondable_request<E, R>(
        &mut self,
        hdr: RpcHeader,
        max_chunk_size: usize,
        on_emit: E,
        on_response: Option<R>,
        prebuffer_response: bool,
    ) -> Result<RpcStreamEncoder<E>, FrameEncodeError>
    where
        E: RpcEmit,
        R: RpcResponseHandler + 'a,
    {
        let rpc_request_id = hdr.rpc_request_id;

        // Set pre-buffering flag for this specific request
        self.prebuffering_flags
            .insert(rpc_request_id, prebuffer_response);

        if let Some(on_response) = on_response {
            self.response_handlers
                .insert(rpc_request_id, Box::new(on_response));
        }

        self.rpc_session
            .init_request(hdr, max_chunk_size, on_emit)
            .map_err(|_| FrameEncodeError::CorruptFrame)
    }

    pub fn start_reply_stream<E>(
        &mut self,
        hdr: RpcHeader,
        max_chunk_size: usize,
        on_emit: E,
    ) -> Result<RpcStreamEncoder<E>, FrameEncodeError>
    where
        E: RpcEmit,
    {
        self.rpc_session
            .init_request(hdr, max_chunk_size, on_emit)
            .map_err(|_| FrameEncodeError::CorruptFrame)
    }

    // TODO: Document
    // Invoked on the remote in response to `init_respondable_request` from the local client
    pub fn set_catch_all_response_handler<R>(&mut self, handler: R)
    where
        R: RpcResponseHandler + 'a,
    {
        self.catch_all_response_handler = Some(Box::new(handler));
    }

    pub fn read_bytes(&mut self, bytes: &[u8]) -> Result<(), FrameDecodeError> {
        self.rpc_session.read_bytes(bytes, |evt| {
            let id = match &evt {
                RpcStreamEvent::Header { rpc_request_id, .. } => Some(*rpc_request_id),
                RpcStreamEvent::PayloadChunk { rpc_request_id, .. } => Some(*rpc_request_id),
                RpcStreamEvent::End { rpc_request_id, .. } => Some(*rpc_request_id),
                RpcStreamEvent::Error { rpc_request_id, .. } => *rpc_request_id,
            };

            let method_id = match &evt {
                RpcStreamEvent::Header { rpc_method_id, .. } => Some(*rpc_method_id),
                RpcStreamEvent::PayloadChunk { rpc_method_id, .. } => Some(*rpc_method_id),
                RpcStreamEvent::End { rpc_method_id, .. } => Some(*rpc_method_id),
                RpcStreamEvent::Error { rpc_method_id, .. } => *rpc_method_id,
            };

            let mut handled = false;

            if let Some(rpc_id) = id {
                let is_prebuffering_response = match self.prebuffering_flags.get(&rpc_id) {
                    Some(bool) => bool,
                    None => &false,
                };

                if *is_prebuffering_response {
                    // Accumulate the bytes into the buffer for this request ID
                    let buffer = self
                        .prebuffered_responses
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
                        RpcStreamEvent::End { rpc_header, .. } => {
                            // When the end of the stream is reached, call the response handler
                            if let Some(cb) = self.response_handlers.get_mut(&rpc_id) {
                                let rpc_method_id =
                                    method_id.ok_or(FrameDecodeError::CorruptFrame)?;

                                let rpc_payload_event = RpcStreamEvent::PayloadChunk {
                                    rpc_request_id: rpc_id,
                                    rpc_method_id,
                                    bytes: buffer.clone(),
                                    rpc_header: rpc_header.clone(),
                                };

                                cb(rpc_payload_event);
                                cb(evt.clone());

                                self.prebuffered_responses.remove(&rpc_id); // Clear the buffer after calling
                            }
                        }
                        _ => {
                            tracing::error!("Unknown `RpcStreamEvent`");
                        }
                    };
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

            Ok(())
        })?;

        Ok(())
    }

    pub fn get_remaining_response_handlers(&self) -> usize {
        self.response_handlers.len()
    }
}
