use crate::frame::FrameEncodeError;
use crate::rpc::{
    RpcHeader, RpcMessageType, RpcMethodHandler, RpcMethodRegistry, RpcSessionNode, RpcStreamEvent,
};

/// Dispatcher built on top of `RpcSessionNode`.
///
/// This struct tracks method dispatching for outgoing calls,
/// and manages automatic response handling.
pub struct RpcDispatcher<'a> {
    session: RpcSessionNode<'a>,
    next_id: u32,
    rpc_method_registry: RpcMethodRegistry<'a>,
}

impl<'a> RpcDispatcher<'a> {
    pub fn new(session: RpcSessionNode<'a>) -> Self {
        Self {
            session,
            next_id: 1,
            rpc_method_registry: RpcMethodRegistry::new(),
        }
    }

    pub fn register(&mut self, method_name: &'static str, handler: RpcMethodHandler<'a>) {
        self.rpc_method_registry.register(method_name, handler)
    }

    // TODO: Ensure method can be run async
    /// Call a remote method. Metadata is the UTF-8 method name.
    pub fn call<G>(
        &mut self,
        method: &str,
        args: Vec<u8>, // TODO: Accept real args and convert internally to use metadata
        // TODO: Accept optional payload
        max_chunk_size: usize,
        mut on_emit: G, // TODO: Can this be moved to a "global" emit?
    ) -> Result<(), FrameEncodeError>
    where
        G: FnMut(&[u8]) + 'a,
    {
        let id = self.next_id;
        self.next_id += 1;

        let hdr = RpcHeader {
            msg_type: RpcMessageType::Call,
            id,
            method_id: 0,                               // TODO: Push hashed method
            metadata_bytes: method.as_bytes().to_vec(), // TODO: Push metadata
        };

        let method_name = method.to_string();
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
                            method_name, bytes
                        );
                    }
                }
                RpcStreamEvent::End { .. } => {
                    println!("Call to {} completed", method_name);
                }
                RpcStreamEvent::Error {
                    frame_decode_error, ..
                } => {
                    eprintln!("Error in call to {}: {:?}", method_name, frame_decode_error);
                }
            }) as Box<dyn FnMut(RpcStreamEvent) + 'a>
        };

        let mut encoder =
            self.session
                .init_request(hdr, max_chunk_size, &mut on_emit, Some(on_response))?;

        // encoder.push_bytes(&args)?; // TODO: Push args as metadata
        encoder.flush()?;
        encoder.end_stream()?;

        Ok(())
    }

    // Expose internal session mutably if needed.
    // pub fn session_mut(&mut self) -> &mut RpcSessionNode<'a> {
    //     &mut self.session
    // }
}
