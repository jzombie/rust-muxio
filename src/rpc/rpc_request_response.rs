#[derive(PartialEq, Debug)]
pub struct RpcRequest {
    pub method_id: u64,
    pub param_bytes: Option<Vec<u8>>,
    pub pre_buffered_payload_bytes: Option<Vec<u8>>,
    pub is_finalized: bool,
}

#[derive(PartialEq, Debug)]
pub struct RpcResponse {
    pub request_header_id: u32,
    pub pre_buffered_payload_bytes: Option<Vec<u8>>,
    pub is_finalized: bool,
}
