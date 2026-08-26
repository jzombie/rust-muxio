use crate::frame::{FrameDecodeError, FrameEncodeError};
use crate::rpc::rpc_internals::{
    RpcHeader, RpcSession, RpcStreamEncoder, RpcStreamEvent,
    rpc_trait::{RpcEmit, RpcResponseHandler, RpcStreamMethodRouter},
};
use crate::utils::IdSpace;
use std::collections::HashMap;

impl<'a> Default for RpcRespondableSession<'a> {
    fn default() -> Self {
        Self::new(IdSpace::Client)
    }
}

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
    pub(crate) response_handlers: HashMap<u32, Box<dyn FnMut(RpcStreamEvent) + Send + 'a>>,
    catch_all_response_handler: Option<Box<dyn FnMut(RpcStreamEvent) + Send + 'a>>,
    pub(crate) prebuffered_responses: HashMap<u32, Vec<u8>>, // Track buffered responses by request ID
    pub(crate) prebuffering_flags: HashMap<u32, bool>, // Track whether pre-buffering is enabled for each request
    /// Optional router that maps (method_id, request_id) to a per-request handler.
    /// When set, it is consulted on each `Header` event. If it returns a handler,
    /// the handler is registered for that stream, bypassing the catch-all accumulator.
    /// This enables streaming handler dispatch on the endpoint side.
    stream_method_router: Option<RpcStreamMethodRouter<'a>>,
}

impl<'a> RpcRespondableSession<'a> {
    pub fn new(id_space: IdSpace) -> Self {
        Self {
            rpc_session: RpcSession::new(id_space),
            response_handlers: HashMap::new(),
            catch_all_response_handler: None,
            prebuffered_responses: HashMap::new(),
            prebuffering_flags: HashMap::new(),
            stream_method_router: None,
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

    /// Sets a router function for streaming method dispatch.
    ///
    /// When a `Header` event arrives and no per-request handler exists yet,
    /// the router is called with `(method_id, request_id)`. If it returns a
    /// handler, that handler is registered for the stream's lifetime.
    /// Subsequent events (`PayloadChunk`, `End`) are routed to the per-request
    /// handler instead of the catch-all accumulator.
    pub fn set_stream_method_router<R>(&mut self, router: R)
    where
        R: FnMut(u64, u32) -> Option<Box<dyn FnMut(RpcStreamEvent) + Send + 'a>> + Send + 'a,
    {
        self.stream_method_router = Some(Box::new(router));
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
            // --- Streaming method routing ---
            // If a Header event arrives for a streaming method, register a
            // per-request handler BEFORE the normal routing so subsequent
            // events bypass the catch-all accumulator.
            if let RpcStreamEvent::Header {
                rpc_request_id,
                rpc_method_id,
                ..
            } = &evt
                && let Some(ref mut router) = self.stream_method_router
                && let Some(handler) = router(*rpc_method_id, *rpc_request_id)
            {
                self.response_handlers.insert(*rpc_request_id, handler);
            }

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
                let is_prebuffering_response =
                    self.prebuffering_flags.get(&rpc_id).unwrap_or(&false);

                if *is_prebuffering_response {
                    // Accumulate the bytes into the buffer for this request ID
                    let buffer = self.prebuffered_responses.entry(rpc_id).or_default();

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

                                self.prebuffered_responses.remove(&rpc_id);
                                self.prebuffering_flags.remove(&rpc_id);
                            }
                        }
                        _ => {
                            tracing::error!("Unknown `RpcStreamEvent`");
                        }
                    };
                } else if let Some(cb) = self.response_handlers.get_mut(&rpc_id) {
                    cb(evt.clone());
                    handled = true;
                }

                if matches!(
                    evt,
                    RpcStreamEvent::End { .. } | RpcStreamEvent::Error { .. }
                ) {
                    self.response_handlers.remove(&rpc_id);
                    self.prebuffering_flags.remove(&rpc_id);
                    self.prebuffered_responses.remove(&rpc_id);
                }
            }

            if !handled && let Some(cb) = self.catch_all_response_handler.as_mut() {
                cb(evt);
            }

            Ok(())
        })?;

        Ok(())
    }

    pub fn get_remaining_response_handlers(&self) -> usize {
        self.response_handlers.len()
    }

    pub(crate) fn clear_all_prebuffering(&mut self) {
        self.prebuffering_flags.clear();
        self.prebuffered_responses.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::rpc_internals::{RpcHeader, RpcMessageType, RpcStreamEvent};
    use std::sync::{Arc, Mutex};

    #[test]
    fn prebuffering_flags_cleared_on_end() {
        // Drive the actual state machine: client -> server -> client roundtrip with prebuffering=true
        let client = Arc::new(Mutex::new(RpcRespondableSession::new(
            crate::utils::IdSpace::Client,
        )));
        let server = Arc::new(Mutex::new(RpcRespondableSession::new(
            crate::utils::IdSpace::Server,
        )));

        let mut server_inbox = Vec::new();
        let client_inbox = Arc::new(Mutex::new(Vec::new()));

        let call_header = RpcHeader {
            rpc_msg_type: RpcMessageType::Call,
            rpc_request_id: 1,
            rpc_method_id: 42,
            rpc_metadata_bytes: vec![],
        };

        // Server will reply with a single chunk and End
        let pending = Arc::new(Mutex::new(Vec::new()));
        {
            let pending_clone = Arc::clone(&pending);
            server
                .lock()
                .unwrap()
                .set_catch_all_response_handler(move |evt| {
                    if let RpcStreamEvent::End { rpc_request_id, .. } = evt {
                        let reply_header = RpcHeader {
                            rpc_msg_type: RpcMessageType::Response,
                            rpc_request_id,
                            rpc_method_id: 42,
                            rpc_metadata_bytes: vec![],
                        };
                        pending_clone.lock().unwrap().push(reply_header);
                    }
                });
        }

        let mut client_enc = client
            .lock()
            .unwrap()
            .init_respondable_request(
                call_header,
                1024,
                |bytes| server_inbox.push(bytes.to_vec()),
                Some(Box::new(|_| {})),
                true,
            )
            .expect("init");
        client_enc.write_bytes(b"ping").unwrap();
        client_enc.flush().unwrap();
        client_enc.end_stream().unwrap();

        for chunk in &server_inbox {
            server.lock().unwrap().read_bytes(chunk).unwrap();
        }
        // Server has processed the Call and queued a reply header
        assert_eq!(client.lock().unwrap().prebuffering_flags.len(), 1);
        // Now create the actual reply bytes from the pending header and feed them to the client
        for reply_header in pending.lock().unwrap().drain(..) {
            let mut enc = server
                .lock()
                .unwrap()
                .start_reply_stream(reply_header, 1024, |bytes| {
                    client_inbox.lock().unwrap().push(bytes.to_vec())
                })
                .expect("server reply");
            enc.write_bytes(b"hello").unwrap();
            enc.flush().unwrap();
            enc.end_stream().unwrap();
        }
        for chunk in client_inbox.lock().unwrap().iter() {
            client.lock().unwrap().read_bytes(chunk).unwrap();
        }
        // After processing End, the prebuffering maps must be automatically cleared
        let guard = client.lock().unwrap();
        assert!(
            guard.prebuffering_flags.is_empty(),
            "prebuffering_flags should be cleared after End"
        );
        assert!(
            guard.prebuffered_responses.is_empty(),
            "prebuffered_responses should be cleared after End"
        );
        assert_eq!(guard.get_remaining_response_handlers(), 0);
    }

    #[test]
    fn prebuffering_flags_cleared_on_error() {
        // Verify that an Error event (e.g., transport failure) clears the
        // prebuffering state via the actual state machine, not just the helper.
        // We use RpcDispatcher's fail_all path which synthesizes an Error event
        // for the pending request and should clear both maps.
        use crate::rpc::RpcDispatcher;
        use crate::rpc::RpcRequest;

        let mut dispatcher = RpcDispatcher::new();
        let req = RpcRequest {
            rpc_method_id: 43,
            rpc_param_bytes: None,
            rpc_prebuffered_payload_bytes: None,
            is_finalized: false,
        };
        let _enc = dispatcher
            .call(req, 1024, |_: &[u8]| {}, Some(Box::new(|_| {})), true)
            .expect("init");
        assert_eq!(
            dispatcher.rpc_respondable_session.prebuffering_flags.len(),
            1
        );
        dispatcher.fail_all_pending_requests(crate::frame::FrameDecodeError::Transport(
            "test".to_string(),
        ));
        assert!(
            dispatcher
                .rpc_respondable_session
                .prebuffering_flags
                .is_empty(),
            "prebuffering_flags should be cleared after Error via fail_all"
        );
        assert!(
            dispatcher
                .rpc_respondable_session
                .prebuffered_responses
                .is_empty(),
            "prebuffered_responses should be cleared after Error via fail_all"
        );
    }
}
