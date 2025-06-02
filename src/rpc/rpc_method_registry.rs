use crate::rpc::{RpcHeader, RpcStreamEvent};
use bitcode::{Decode, Encode};
use std::collections::HashMap;
use xxhash_rust::xxh3::xxh3_64;

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct RpcRequest {
    pub method: String,
    pub param_bytes: Vec<u8>, // The serialized params
                              // pub id: Option<u64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct RpcResponse {
    pub result: Vec<u8>,
    // pub id: Option<u64>,
}

pub type RpcMethodHandler<'a> =
    Box<dyn FnMut(RpcHeader, Vec<u8>, Box<dyn FnMut(RpcStreamEvent) + 'a>) + 'a>;

pub struct RpcMethodRegistry<'a> {
    handlers: HashMap<u64, RpcMethodHandler<'a>>,
    name_to_id: HashMap<&'static str, u64>,
    id_to_name: HashMap<u64, &'static str>,
}

impl<'a> RpcMethodRegistry<'a> {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            name_to_id: HashMap::new(),
            id_to_name: HashMap::new(),
        }
    }

    pub fn register(&mut self, method_name: &'static str, handler: RpcMethodHandler<'a>) {
        let id = xxh3_64(method_name.as_bytes());
        self.name_to_id.insert(method_name, id);
        self.id_to_name.insert(id, method_name);
        self.handlers.insert(id, handler);
    }

    pub fn get(&mut self, method_id: u64) -> Option<&mut RpcMethodHandler<'a>> {
        self.handlers.get_mut(&method_id)
    }
}

// // Example handler for Echo method
// fn echo_handler(params: Vec<u8>) -> Result<String, String> {
//     let params: EchoParams = deserialize(&params).map_err(|e| e.to_string())?;
//     Ok(params.text)
// }

// // Dispatcher calling the handler with params
// pub fn dispatch_rpc(req: RpcRequest) -> RpcResponse {
//     let result = match req.method.as_str() {
//         "echo" => {
//             // Deserialize parameters for the echo method
//             let response = echo_handler(req.params);
//             response.unwrap_or_else(|e| serde_json::json!({ "error": e }).to_string())
//         }
//         _ => "Unknown method".to_string(),
//     };

//     RpcResponse {
//         result: result.into(),
//         id: req.id,
//     }
// }
