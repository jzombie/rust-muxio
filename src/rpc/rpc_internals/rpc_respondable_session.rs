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
    response_handlers: HashMap<u32, Box<dyn FnMut(RpcStreamEvent) + 'a>>,
    catch_all_response_handler: Option<Box<dyn FnMut(RpcStreamEvent) + 'a>>,
}

impl<'a> RpcRespondableSession<'a> {
    pub fn new() -> Self {
        Self {
            rpc_session: RpcSession::new(),
            response_handlers: HashMap::new(),
            catch_all_response_handler: None,
        }
    }

    pub fn init_respondable_request<G, F>(
        &mut self,
        hdr: RpcHeader,
        max_chunk_size: usize,
        on_emit: G,
        on_response: Option<F>,
    ) -> Result<RpcStreamEncoder<G>, FrameEncodeError>
    where
        G: FnMut(&[u8]),
        F: FnMut(RpcStreamEvent) + 'a,
    {
        let rpc_header_id = hdr.id;

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
        F: FnMut(RpcStreamEvent) + 'a,
    {
        self.catch_all_response_handler = Some(Box::new(handler));
    }

    pub fn receive_bytes(&mut self, bytes: &[u8]) -> Result<(), FrameDecodeError> {
        self.rpc_session.receive_bytes(bytes, |evt| {
            let id = match &evt {
                RpcStreamEvent::Header { rpc_header_id, .. } => Some(*rpc_header_id),
                RpcStreamEvent::PayloadChunk { rpc_header_id, .. } => Some(*rpc_header_id),
                RpcStreamEvent::End { rpc_header_id } => Some(*rpc_header_id),
                RpcStreamEvent::Error { rpc_header_id, .. } => *rpc_header_id,
            };

            let mut handled = false;

            if let Some(rpc_id) = id {
                if let Some(cb) = self.response_handlers.get_mut(&rpc_id) {
                    cb(evt.clone());
                    handled = true;
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
