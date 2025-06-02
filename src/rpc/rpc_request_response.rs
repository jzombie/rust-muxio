#[derive(PartialEq, Debug)]
pub struct RpcRequest {
    pub method_name: String,
    pub param_bytes: Vec<u8>,
    pub pre_buffered_payload_bytes: Option<Vec<u8>>, // TODO: Handle streaming payload requests
    pub is_finalized: bool,
}

// TODO: Remove?
// #[derive(PartialEq, Debug)]
// pub struct RpcResponse {
//     pub result: Vec<u8>,
// }
