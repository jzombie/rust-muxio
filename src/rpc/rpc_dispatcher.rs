use crate::frame::{FrameDecodeError, FrameEncodeError};
use crate::rpc::{
    RpcHeader, RpcMessageType, RpcMethodHandler, RpcMethodRegistry, RpcRequest, RpcSessionNode,
    RpcStreamEncoder, RpcStreamEvent,
};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

pub struct RpcDispatcher<'a> {
    rpc_session: Rc<RefCell<RpcSessionNode<'a>>>,
    next_id: u32, // TODO: Use parent?
    rpc_method_registry: RpcMethodRegistry<'a>,
    rpc_request_queue: Rc<RefCell<VecDeque<(u32, RpcRequest)>>>,
}

impl<'a> RpcDispatcher<'a> {
    pub fn new() -> Self {
        let rpc_session = Rc::new(RefCell::new(RpcSessionNode::new()));

        let instance = Self {
            rpc_session,
            next_id: 1, // TODO: Use parent?
            rpc_method_registry: RpcMethodRegistry::new(),
            rpc_request_queue: Rc::new(RefCell::new(VecDeque::new())),
        };

        instance.init_catch_all_response_handler();

        instance
    }

    fn init_catch_all_response_handler(&self) {
        let rpc_request_queue_ref = Rc::clone(&self.rpc_request_queue);

        self.rpc_session
            .borrow_mut()
            .set_catch_all_response_handler(Box::new(move |event: RpcStreamEvent| {
                match event {
                    RpcStreamEvent::Header {
                        rpc_header_id,
                        rpc_header,
                    } => {
                        // Create a new RpcRequest with the header's method name and metadata
                        let method_name =
                            String::from_utf8_lossy(&rpc_header.metadata_bytes).to_string();
                        let rpc_request = RpcRequest {
                            method_name,
                            param_bytes: rpc_header.metadata_bytes.clone(),
                            payload_bytes: None, // No payload yet
                        };

                        // Push the RpcRequest into the queue
                        println!("Received Header: {} - {:?}", rpc_header_id, rpc_header);
                        rpc_request_queue_ref
                            .borrow_mut()
                            .push_back((rpc_header_id, rpc_request));
                    }

                    RpcStreamEvent::PayloadChunk {
                        rpc_header_id,
                        bytes,
                    } => {
                        // If we have an existing RpcRequest in the queue, we append the payload
                        if let Some((header_id, mut rpc_request)) =
                            rpc_request_queue_ref.borrow_mut().pop_back()
                        {
                            if header_id == rpc_header_id {
                                // Append bytes to the payload
                                let payload =
                                    rpc_request.payload_bytes.get_or_insert_with(Vec::new);
                                payload.extend_from_slice(&bytes);

                                // Push the updated RpcRequest back into the queue
                                rpc_request_queue_ref
                                    .borrow_mut()
                                    .push_back((header_id, rpc_request));
                            }
                        } else {
                            // Handle the case where there's no corresponding header in the queue
                            eprintln!("No header found for payload with ID: {}", rpc_header_id);
                        }
                    }

                    RpcStreamEvent::End { rpc_header_id } => {
                        // Finalize and process the full message when the stream ends
                        if let Some((header_id, rpc_request)) =
                            rpc_request_queue_ref.borrow_mut().pop_back()
                        {
                            if header_id == rpc_header_id {
                                // Now we have the full message (header + payload)
                                println!("Stream End: {} with complete payload", rpc_header_id);
                                println!("Complete message: {:?}", rpc_request);
                            }
                        }
                    }

                    RpcStreamEvent::Error {
                        rpc_header_id,
                        frame_decode_error,
                    } => {
                        println!(
                            "Error in stream {:?}: {:?}",
                            rpc_header_id, frame_decode_error
                        );
                    }
                }
            }));
    }

    pub fn start_reply_stream<F>(
        &mut self,
        hdr: RpcHeader,
        max_chunk_size: usize,
        on_emit: F,
    ) -> Result<RpcStreamEncoder<F>, FrameDecodeError>
    where
        F: FnMut(&[u8]),
    {
        self.rpc_session
            .borrow_mut()
            .start_reply_stream(hdr, max_chunk_size, on_emit)
    }

    pub fn register(&mut self, method_name: &'static str, handler: RpcMethodHandler<'a>) {
        self.rpc_method_registry.register(method_name, handler);
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
        let id = self.next_id;
        self.next_id += 1; // TODO: Use parent?

        let hdr = RpcHeader {
            msg_type: RpcMessageType::Call,
            id,
            method_id: 0, // Placeholder method_id, should be computed or set
            metadata_bytes: rpc_request.param_bytes,
        };

        // let on_response = {
        //     let mut seen_start = false;
        //     Box::new(move |event: RpcStreamEvent| match event {
        //         RpcStreamEvent::Header { .. } => {
        //             seen_start = true;
        //         }
        //         RpcStreamEvent::PayloadChunk {
        //             rpc_header_id,
        //             bytes,
        //         } => {
        //             if seen_start {
        //                 println!(
        //                     "Received response payload from {}: {:?}",
        //                     rpc_request.method, bytes
        //                 );
        //             }
        //         }
        //         RpcStreamEvent::End { .. } => {
        //             println!("Call to {} completed", rpc_request.method);
        //         }
        //         RpcStreamEvent::Error {
        //             frame_decode_error, ..
        //         } => {
        //             eprintln!(
        //                 "Error in call to {}: {:?}",
        //                 rpc_request.method, frame_decode_error
        //             );
        //         }
        //     }) as Box<dyn FnMut(RpcStreamEvent) + 'a>
        // };

        // Directly pass the closure as `on_emit` without borrowing it
        let mut encoder = self.rpc_session.borrow_mut().init_request(
            hdr,
            max_chunk_size,
            on_emit,
            on_response,
        )?;

        encoder.push_bytes(b"testing 1 2 3")?;

        encoder.flush()?;
        encoder.end_stream()?;

        Ok(encoder)
    }

    // TODO: Return tasks to perform
    pub fn receive_bytes(&mut self, bytes: &[u8]) -> Result<(), FrameDecodeError> {
        // TODO: Remove
        // println!("BEFORE RECEIVE BYTES QUEUE: {:?}", self.response_queue);

        let resp = self.rpc_session.borrow_mut().receive_bytes(bytes);

        // Capture all events in a vector
        // let captured_events: Vec<RpcStreamEvent> = self.response_queue.borrow_mut().drain(..).collect();

        // TODO: Remove
        // println!("AFTER RECEIVE BYTES QUEUE: {:?}", self.response_queue);

        resp
    }
}
