use crate::{frame::FrameDecodeError, rpc::rpc_internals::RpcHeader};
use std::sync::Arc;

// TODO: Document
#[derive(Debug, Clone)]
pub enum RpcStreamEvent {
    Header {
        rpc_request_id: u32,
        rpc_method_id: u64,
        rpc_header: Arc<RpcHeader>,
    },
    PayloadChunk {
        rpc_header: Arc<RpcHeader>,
        rpc_request_id: u32,
        rpc_method_id: u64,
        bytes: Vec<u8>,
    },
    End {
        rpc_header: Arc<RpcHeader>,
        rpc_request_id: u32,
        rpc_method_id: u64,
    },
    // TODO: Beware that nothing is actually setting these option types as it stands
    Error {
        rpc_header: Option<Arc<RpcHeader>>,
        rpc_request_id: Option<u32>,
        rpc_method_id: Option<u64>,
        frame_decode_error: FrameDecodeError,
    },
}
