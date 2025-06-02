use crate::frame::{FrameDecodeError, FrameEncodeError};
use crate::rpc::{
    RpcHeader, RpcMessageType, RpcMethodHandler, RpcMethodRegistry, RpcRequest, RpcSessionNode,
    RpcStreamEncoder, RpcStreamEvent,
};

pub struct RpcDispatcher<'a> {
    session: RpcSessionNode<'a>,
    next_id: u32, // TODO: Use parent?
    rpc_method_registry: RpcMethodRegistry<'a>,
}

impl<'a> RpcDispatcher<'a> {
    pub fn new() -> Self {
        let mut session = RpcSessionNode::new();

        session.set_response_handler(Box::new(|event: RpcStreamEvent| match event {
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
        }));

        Self {
            session,
            next_id: 1, // TODO: Use parent?
            rpc_method_registry: RpcMethodRegistry::new(),
        }
    }

    pub fn register(&mut self, method_name: &'static str, handler: RpcMethodHandler<'a>) {
        self.rpc_method_registry.register(method_name, handler);
    }

    pub fn call<G>(
        &mut self,
        rpc_request: RpcRequest,
        max_chunk_size: usize,
        on_emit: G, // Directly pass the closure, not as a reference
    ) -> Result<RpcStreamEncoder<G>, FrameEncodeError>
    where
        G: FnMut(&[u8]), // Ensuring that `G` is FnMut(&[u8])
    {
        let id = self.next_id;
        self.next_id += 1; // TODO: Use parent?

        let hdr = RpcHeader {
            msg_type: RpcMessageType::Call,
            id,
            method_id: 0, // Placeholder method_id, should be computed or set
            metadata_bytes: rpc_request.param_bytes,
        };

        let on_response = {
            let mut seen_start = false;
            Box::new(move |event: RpcStreamEvent| match event {
                RpcStreamEvent::Header { .. } => {
                    seen_start = true;
                }
                RpcStreamEvent::PayloadChunk {
                    rpc_header_id,
                    bytes,
                } => {
                    if seen_start {
                        println!(
                            "Received response payload from {}: {:?}",
                            rpc_request.method, bytes
                        );
                    }
                }
                RpcStreamEvent::End { .. } => {
                    println!("Call to {} completed", rpc_request.method);
                }
                RpcStreamEvent::Error {
                    frame_decode_error, ..
                } => {
                    eprintln!(
                        "Error in call to {}: {:?}",
                        rpc_request.method, frame_decode_error
                    );
                }
            }) as Box<dyn FnMut(RpcStreamEvent) + 'a>
        };

        // Directly pass the closure as `on_emit` without borrowing it
        let mut encoder =
            self.session
                .init_request(hdr, max_chunk_size, on_emit, Some(on_response))?;

        encoder.push_bytes(b"testing 1 2 3")?;

        encoder.flush()?;
        encoder.end_stream()?;

        Ok(encoder)
    }

    pub fn receive_bytes(&mut self, bytes: &[u8]) -> Result<(), FrameDecodeError> {
        self.session.receive_bytes(bytes)
    }
}
