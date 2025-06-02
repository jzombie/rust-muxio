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
    // TODO: Integrate (and not public)
    pub response_queue: Rc<RefCell<VecDeque<u8>>>,
}

impl<'a> RpcDispatcher<'a> {
    pub fn new() -> Self {
        let rpc_session = Rc::new(RefCell::new(RpcSessionNode::new()));

        let instance = Self {
            rpc_session,
            next_id: 1, // TODO: Use parent?
            rpc_method_registry: RpcMethodRegistry::new(),
            response_queue: Rc::new(RefCell::new(VecDeque::new())),
        };

        instance.init_catch_all_response_handler();

        instance
    }

    fn init_catch_all_response_handler(&self) {
        // Use a clone of the Rc to move it into the closure, allowing mutable access
        // let session_ref = Rc::clone(&self.session);

        let reponse_queue_ref = Rc::clone(&self.response_queue);

        self.rpc_session
            .borrow_mut()
            .set_catch_all_response_handler(Box::new(move |event: RpcStreamEvent| {
                // Handle the event here
                match event {
                    RpcStreamEvent::Header {
                        rpc_header_id,
                        rpc_header,
                    } => {
                        println!("Received Header: {} - {:?}", rpc_header_id, rpc_header);
                    }
                    RpcStreamEvent::PayloadChunk {
                        rpc_header_id,
                        bytes,
                    } => {
                        println!("Received Payload: {} - {:?}", rpc_header_id, bytes);
                    }
                    RpcStreamEvent::End { rpc_header_id } => {
                        println!("Stream End: {}", rpc_header_id);

                        // TODO: Push methods to a queue
                        reponse_queue_ref.borrow_mut().push_back(1);

                        // Borrow mutably and use session to start reply stream
                        // RpcDispatcher::start_reply_stream(
                        //     RpcHeader {
                        //         msg_type: RpcMessageType::Response,
                        //         id: rpc_header_id,
                        //         method_id: 0,
                        //         metadata_bytes: vec![],
                        //     },
                        //     4,
                        //     |bytes: &[u8]| {
                        //         // Handle reply bytes here
                        //         println!("Reply bytes: {:?}", bytes);
                        //     },
                        // )
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
        println!("BEFORE RECEIVE BYTES QUEUE: {:?}", self.response_queue);

        let resp = self.rpc_session.borrow_mut().receive_bytes(bytes);

        // TODO: Remove
        println!("AFTER RECEIVE BYTES QUEUE: {:?}", self.response_queue);

        resp
    }
}
