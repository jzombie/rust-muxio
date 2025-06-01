use crate::{frame::FrameDecodeError, rpc::RpcHeader};

// TODO: Rename `rpc_header_id`
pub enum RpcStreamEvent {
    Header {
        rpc_header_id: u32,
        rpc_header: RpcHeader,
    },
    PayloadChunk {
        rpc_header_id: u32,
        bytes: Vec<u8>,
    },
    Error {
        rpc_header_id: Option<u32>,
        frame_decode_error: FrameDecodeError,
    },
    End {
        rpc_header_id: u32,
    },
}
