// use xxhash_rust::xxh3::xxh3_64;

#[derive(PartialEq, Debug)]
pub struct RpcRequest {
    pub method_id: u64,
    pub param_bytes: Option<Vec<u8>>,
    pub pre_buffered_payload_bytes: Option<Vec<u8>>,
    pub is_finalized: bool,
}

// TODO: Re-add?
// impl RpcRequest {
//     pub fn to_method_id(method_name: &str) -> u64 {
//         xxh3_64(method_name.as_bytes())
//     }
// }

#[derive(PartialEq, Debug)]
pub struct RpcResponse {
    pub request_header_id: u32,
    pub pre_buffered_payload_bytes: Option<Vec<u8>>,
    pub is_finalized: bool,
}
