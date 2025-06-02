use crate::rpc::{RpcHeader, RpcStreamEvent};
use std::collections::HashMap;
use xxhash_rust::xxh3::xxh3_64;

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
