use crate::frame::{FrameDecodeError, FrameEncodeError};
use crate::rpc::{
    RpcRequest, RpcResponse,
    rpc_internals::{
        RpcHeader, RpcMessageType, RpcRespondableSession, RpcStreamEncoder, RpcStreamEvent,
    },
};
use crate::utils::increment_u32_id;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub struct RpcDispatcher<'a> {
    rpc_session: RpcRespondableSession<'a>,
    next_header_id: u32,
    rpc_request_queue: Arc<Mutex<VecDeque<(u32, RpcRequest)>>>,
}

impl<'a> RpcDispatcher<'a> {
    pub fn new() -> Self {
        let rpc_session = RpcRespondableSession::new();

        let mut instance = Self {
            rpc_session,
            next_header_id: increment_u32_id(),
            rpc_request_queue: Arc::new(Mutex::new(VecDeque::new())),
        };

        instance.init_catch_all_response_handler();

        instance
    }

    // Invoked on the remote in reponse to `call` requests from the local client
    fn init_catch_all_response_handler(&mut self) {
        let rpc_request_queue_ref = Arc::clone(&self.rpc_request_queue);

        self.rpc_session
            .set_catch_all_response_handler(Box::new(move |event: RpcStreamEvent| {
                let mut queue = rpc_request_queue_ref.lock().unwrap();

                // TODO: Delete from queue if request is canceled mid-flight
                match event {
                    RpcStreamEvent::Header {
                        rpc_header_id,
                        rpc_header,
                        ..
                    } => {
                        // Convert metadata to parameter bytes
                        let param_bytes = match rpc_header.metadata_bytes.len() {
                            0 => None,
                            _ => Some(rpc_header.metadata_bytes),
                        };

                        let rpc_request = RpcRequest {
                            method_id: rpc_header.method_id,
                            param_bytes,
                            pre_buffered_payload_bytes: None, // No payload yet
                            is_finalized: false,
                        };

                        queue.push_back((rpc_header_id, rpc_request));
                    }

                    RpcStreamEvent::PayloadChunk {
                        rpc_header_id,
                        bytes,
                        ..
                    } => {
                        // Look for the existing RpcRequest in the queue and borrow it mutably
                        if let Some((_, rpc_request)) =
                            queue.iter_mut().find(|(id, _)| *id == rpc_header_id)
                        {
                            // Append bytes to the payload
                            let payload = rpc_request
                                .pre_buffered_payload_bytes
                                .get_or_insert_with(Vec::new);
                            payload.extend_from_slice(&bytes);
                        }
                    }

                    RpcStreamEvent::End { rpc_header_id, .. } => {
                        // Finalize and process the full message when the stream ends
                        if let Some((_, rpc_request)) =
                            queue.iter_mut().find(|(id, _)| *id == rpc_header_id)
                        {
                            // Set the `is_finalized` flag to true when the stream ends
                            rpc_request.is_finalized = true;
                        }
                    }

                    RpcStreamEvent::Error {
                        rpc_header_id,
                        rpc_method_id,
                        frame_decode_error,
                    } => {
                        // TODO: Handle errors
                        println!(
                            "Error in stream. Method: {:?} {:?}: {:?}",
                            rpc_method_id, rpc_header_id, frame_decode_error
                        );
                    }
                }
            }));
    }

    pub fn call<G, F>(
        &mut self,
        rpc_request: RpcRequest,
        max_chunk_size: usize,
        on_emit: G, // Directly pass the closure, not as a reference
        on_response: Option<F>,
        pre_buffer_response: bool,
    ) -> Result<RpcStreamEncoder<G>, FrameEncodeError>
    where
        G: FnMut(&[u8]), // Ensuring that `G` is FnMut(&[u8])
        F: FnMut(RpcStreamEvent) + Send + 'a,
    {
        let method_id = rpc_request.method_id;

        let header_id: u32 = self.next_header_id;
        self.next_header_id = increment_u32_id();

        // Convert parameter bytes to metadata
        let metadata_bytes = match rpc_request.param_bytes {
            Some(param_bytes) => param_bytes,
            None => vec![],
        };

        let request_header = RpcHeader {
            msg_type: RpcMessageType::Call,
            id: header_id,
            method_id,
            metadata_bytes,
        };

        // Directly pass the closure as `on_emit` without borrowing it
        let mut encoder = self.rpc_session.init_respondable_request(
            request_header,
            max_chunk_size,
            on_emit,
            on_response,
            pre_buffer_response,
        )?;

        // If the RPC request has a buffered payload, send it here
        if let Some(pre_buffered_payload_bytes) = rpc_request.pre_buffered_payload_bytes {
            encoder.push_bytes(&pre_buffered_payload_bytes)?;
        }

        // If the RPC request is pre-finalized, close the stream
        if rpc_request.is_finalized {
            encoder.flush()?;
            encoder.end_stream()?;
        }

        Ok(encoder)
    }

    pub fn respond<F>(
        &mut self,
        rpc_response: RpcResponse,
        max_chunk_size: usize,
        on_emit: F,
    ) -> Result<RpcStreamEncoder<F>, FrameEncodeError>
    where
        F: FnMut(&[u8]),
    {
        let rpc_response_header = RpcHeader {
            id: rpc_response.request_header_id,
            msg_type: RpcMessageType::Response,
            method_id: rpc_response.method_id,
            metadata_bytes: {
                match rpc_response.result_status {
                    Some(result_status) => vec![result_status],
                    None => vec![],
                }
            },
        };

        let mut response_encoder =
            self.rpc_session
                .start_reply_stream(rpc_response_header, max_chunk_size, on_emit)?;

        if let Some(pre_buffered_payload_bytes) = rpc_response.pre_buffered_payload_bytes {
            response_encoder.push_bytes(&pre_buffered_payload_bytes)?;
        }

        if rpc_response.is_finalized {
            response_encoder.flush()?;
            response_encoder.end_stream()?;
        }

        Ok(response_encoder)
    }

    pub fn receive_bytes(&mut self, bytes: &[u8]) -> Result<Vec<u32>, FrameDecodeError> {
        // Process the incoming bytes
        self.rpc_session.receive_bytes(bytes)?;

        // List of request header IDs which are currently in progress
        let queue = self
            .rpc_request_queue
            .lock()
            .map_err(|_| FrameDecodeError::CorruptFrame)?;
        let active_request_header_ids: Vec<u32> =
            queue.iter().map(|(header_id, _)| *header_id).collect();

        Ok(active_request_header_ids)
    }

    pub fn get_rpc_request(
        &self,
        header_id: u32,
    ) -> Option<std::sync::MutexGuard<'_, VecDeque<(u32, RpcRequest)>>> {
        let queue = self.rpc_request_queue.lock().ok()?;

        // You can't return a reference to an element inside a MutexGuard
        // so we return the full guard instead
        if queue.iter().any(|(id, _)| *id == header_id) {
            Some(queue) // Caller must search again within the guard
        } else {
            None
        }
    }

    pub fn is_rpc_request_finalized(&self, header_id: u32) -> Option<bool> {
        let queue = self.rpc_request_queue.lock().ok()?;
        queue
            .iter()
            .find(|(id, _)| *id == header_id)
            .map(|(_, req)| req.is_finalized)
    }

    // Deletes the request and transfers ownership to the caller
    pub fn delete_rpc_request(&self, header_id: u32) -> Option<RpcRequest> {
        let mut queue = self.rpc_request_queue.lock().ok()?;

        if let Some(index) = queue.iter().position(|(id, _)| *id == header_id) {
            // Remove and return just the RpcRequest
            Some(queue.remove(index)?.1)
        } else {
            None
        }
    }
}
