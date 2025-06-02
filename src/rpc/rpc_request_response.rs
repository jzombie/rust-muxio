use xxhash_rust::xxh3::xxh3_64;

#[derive(PartialEq, Debug)]
pub struct RpcRequest {
    pub method_name: String,
    pub param_bytes: Option<Vec<u8>>,
    pub pre_buffered_payload_bytes: Option<Vec<u8>>,
    pub is_finalized: bool,
}

impl RpcRequest {
    pub fn to_method_id(&self) -> u64 {
        xxh3_64(self.method_name.as_bytes())
    }
}

#[derive(PartialEq, Debug)]
pub struct RpcResponse {
    pub request_header_id: u32,
    pub pre_buffered_payload_bytes: Option<Vec<u8>>,
    pub is_finalized: bool,
}
