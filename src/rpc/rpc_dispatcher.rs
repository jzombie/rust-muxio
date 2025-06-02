use crate::frame::{FrameDecodeError, FrameEncodeError};
use crate::rpc::{
    RpcRequest, RpcResponse,
    rpc_internals::{
        RpcHeader, RpcMessageType, RpcRespondableSession, RpcStreamEncoder, RpcStreamEvent,
    },
};
use std::cell::Ref;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

pub struct RpcDispatcher<'a> {
    rpc_session: RpcRespondableSession<'a>,
    next_header_id: u32,
    rpc_request_queue: Rc<RefCell<VecDeque<(u32, RpcRequest)>>>,
}

impl<'a> RpcDispatcher<'a> {
    pub fn new() -> Self {
        let rpc_session = RpcRespondableSession::new();

        let mut instance = Self {
            rpc_session,
            next_header_id: 1,
            rpc_request_queue: Rc::new(RefCell::new(VecDeque::new())),
        };

        instance.init_catch_all_response_handler();

        instance
    }

    fn init_catch_all_response_handler(&mut self) {
        let rpc_request_queue_ref = Rc::clone(&self.rpc_request_queue);

        self.rpc_session
            .set_catch_all_response_handler(Box::new(move |event: RpcStreamEvent| {
                let mut queue = rpc_request_queue_ref.borrow_mut(); // Borrow once

                // TODO: Delete from queue if request is canceled mid-flight
                match event {
                    RpcStreamEvent::Header {
                        rpc_header_id,
                        rpc_header,
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

                    RpcStreamEvent::End { rpc_header_id } => {
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
                        frame_decode_error,
                    } => {
                        // TODO: Handle errors
                        println!(
                            "Error in stream {:?}: {:?}",
                            rpc_header_id, frame_decode_error
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
    ) -> Result<RpcStreamEncoder<G>, FrameEncodeError>
    where
        G: FnMut(&[u8]), // Ensuring that `G` is FnMut(&[u8])
        F: FnMut(RpcStreamEvent) + 'a,
    {
        let method_id = rpc_request.method_id;
        let header_id: u32 = self.next_header_id;
        self.next_header_id += 1;

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
            metadata_bytes: vec![],
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
        let active_request_header_ids: Vec<u32> = self
            .rpc_request_queue
            .borrow_mut()
            .iter()
            // .filter(|(_header_id, request)| request.is_finalized)
            .map(|(header_id, _)| *header_id)
            .collect();

        Ok(active_request_header_ids)
    }

    pub fn get_rpc_request(&self, header_id: u32) -> Option<Ref<RpcRequest>> {
        let queue = self.rpc_request_queue.borrow();

        // Find the index of the matching request
        let index = queue.iter().position(|(id, _)| *id == header_id)?;

        // Now re-borrow with Ref and map to the inner request
        Some(Ref::map(queue, |q| &q[index].1))
    }

    pub fn is_rpc_request_finalized(&self, header_id: u32) -> Option<bool> {
        match self.get_rpc_request(header_id) {
            Some(rpc_request) => Some(rpc_request.is_finalized),
            None => None,
        }
    }

    // Deletes the request and transfers ownership to the caller
    pub fn delete_rpc_request(&self, header_id: u32) -> Option<RpcRequest> {
        let mut queue = self.rpc_request_queue.borrow_mut();

        // Find the index of the matching request and remove it
        if let Some(index) = queue.iter().position(|(id, _)| *id == header_id) {
            // Remove and return the RpcRequest at the found index
            Some(queue.remove(index).map(|(_, request)| request).unwrap())
        } else {
            None // Return None if no matching header_id was found
        }
    }
}
