use crate::{frame::FrameDecodeError, rpc::rpc_internals::RpcHeader};

// TODO: Document
#[derive(Debug, Clone)]
pub enum RpcStreamEvent {
    Header {
        rpc_request_id: u32,
        rpc_method_id: u64,
        rpc_header: RpcHeader,
    },
    PayloadChunk {
        // TODO: For ease of use, add `rpc_header` here?
        rpc_request_id: u32,
        rpc_method_id: u64,
        bytes: Vec<u8>,
    },
    End {
        // TODO: For ease of use, add `rpc_header` here?
        rpc_request_id: u32,
        rpc_method_id: u64,
    },
    Error {
        // TODO: For ease of use, add `Option<RpcHeader>` here?
        rpc_request_id: Option<u32>,
        rpc_method_id: Option<u64>,
        frame_decode_error: FrameDecodeError,
    },
}
