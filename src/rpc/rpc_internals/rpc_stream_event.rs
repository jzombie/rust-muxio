use crate::{frame::FrameDecodeError, rpc::rpc_internals::RpcHeader};

#[derive(Debug, Clone)]
pub enum RpcStreamEvent {
    Header {
        rpc_header_id: u32,
        rpc_method_id: u64,
        rpc_header: RpcHeader,
    },
    PayloadChunk {
        rpc_header_id: u32,
        rpc_method_id: u64,
        bytes: Vec<u8>,
    },
    End {
        rpc_header_id: u32,
        rpc_method_id: u64,
    },
    Error {
        rpc_header_id: Option<u32>,
        rpc_method_id: Option<u64>,
        frame_decode_error: FrameDecodeError,
    },
}
