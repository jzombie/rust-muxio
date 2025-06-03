use crate::rpc::rpc_internals::RpcHeader;
use xxhash_rust::xxh3::xxh3_64;

#[derive(PartialEq, Debug)]
pub struct RpcRequest {
    pub method_id: u64,
    pub param_bytes: Option<Vec<u8>>,
    pub pre_buffered_payload_bytes: Option<Vec<u8>>,
    pub is_finalized: bool,
}

impl RpcRequest {
    pub fn to_method_id(method_name: &str) -> u64 {
        xxh3_64(method_name.as_bytes())
    }
}

#[derive(PartialEq, Debug)]
pub struct RpcResponse {
    pub request_header_id: u32,
    pub method_id: u64,
    pub result_status: Option<u8>,
    pub pre_buffered_payload_bytes: Option<Vec<u8>>,
    pub is_finalized: bool,
}

impl RpcResponse {
    pub fn from_rcp_header(rpc_header: &RpcHeader, is_finalized: bool) -> RpcResponse {
        RpcResponse {
            request_header_id: rpc_header.id,
            method_id: rpc_header.method_id,
            result_status: {
                match rpc_header.metadata_bytes.len() {
                    0 => None,
                    _ => Some(rpc_header.metadata_bytes[0]),
                }
            },
            pre_buffered_payload_bytes: None,
            is_finalized,
        }
    }
}
