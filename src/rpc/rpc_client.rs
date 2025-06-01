use crate::frame::FrameDecodeError;
use crate::rpc::{RpcHeader, RpcMuxSession, RpcStreamEncoder, RpcStreamEvent};
use std::collections::HashMap;

/// A high-level RPC client that routes decoded stream events
/// to per-request response handlers.
pub struct RpcClient<'a> {
    pub(crate) session: RpcMuxSession,
    response_handlers: HashMap<u32, Box<dyn FnMut(RpcStreamEvent) + 'a>>,
}

impl<'a> RpcClient<'a> {
    /// Creates a new RPC client instance.
    pub fn new() -> Self {
        Self {
            session: RpcMuxSession::new(),
            response_handlers: HashMap::new(),
        }
    }

    /// Starts a new outbound RPC stream and registers a response handler.
    ///
    /// This associates the handler with the message ID so that incoming
    /// `RpcStreamEvent`s can be routed correctly.
    pub fn start_rpc_stream<G, F>(
        &mut self,
        hdr: RpcHeader,
        max_payload: usize,
        mut on_emit: G,
        on_response: F,
    ) -> Result<(), FrameDecodeError>
    where
        G: FnMut(&[u8]),
        F: FnMut(RpcStreamEvent) + 'a,
    {
        let msg_id = hdr.id;
        self.response_handlers.insert(msg_id, Box::new(on_response));

        self.session
            .start_rpc_stream(hdr, max_payload, move |bytes| {
                on_emit(bytes);
            })
            .map(|_| ())
            .map_err(|_| FrameDecodeError::CorruptFrame)
    }

    /// Starts a reply stream and returns the encoder so the caller can push data.
    ///
    /// This does not register a handler — typically used for one-shot replies.
    pub fn start_reply_stream<F>(
        &mut self,
        hdr: RpcHeader,
        max_payload: usize,
        on_emit: F,
    ) -> Result<RpcStreamEncoder<F>, FrameDecodeError>
    where
        F: FnMut(&[u8]),
    {
        self.session
            .start_rpc_stream(hdr, max_payload, on_emit)
            .map_err(|_| FrameDecodeError::CorruptFrame)
    }

    /// Feeds in raw bytes and dispatches decoded RPC stream events
    /// to the appropriate handler based on `rpc_header_id`.
    pub fn receive_bytes(&mut self, bytes: &[u8]) -> Result<(), FrameDecodeError> {
        self.session.receive_bytes(bytes, |evt| {
            let target_id = match &evt {
                RpcStreamEvent::Header { rpc_header_id, .. } => Some(*rpc_header_id),
                RpcStreamEvent::PayloadChunk { rpc_header_id, .. } => Some(*rpc_header_id),
                RpcStreamEvent::End { rpc_header_id } => Some(*rpc_header_id),
                RpcStreamEvent::Error { rpc_header_id, .. } => *rpc_header_id,
            };

            if let Some(id) = target_id {
                if let Some(cb) = self.response_handlers.get_mut(&id) {
                    cb(evt);
                }
            }
        })
    }
}
