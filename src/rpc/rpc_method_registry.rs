use crate::rpc::{RpcHeader, RpcStreamEvent};
use std::collections::HashMap;
use xxhash_rust::xxh3::xxh3_64;

/// Function signature for an RPC handler, now including arguments.
pub type RpcMethodHandler<'a> =
    Box<dyn FnMut(RpcHeader, Vec<u8>, Box<dyn FnMut(RpcStreamEvent) + 'a>) + 'a>;

/// A registry mapping method names to their corresponding handler and ID.
pub struct RpcMethodRegistry<'a> {
    /// Maps hashed method ID to handler
    handlers: HashMap<u64, RpcMethodHandler<'a>>,
    /// Maps method name to ID (for lookup and debugging)
    name_to_id: HashMap<&'static str, u64>,
    /// Maps method ID back to name (optional, useful for diagnostics)
    id_to_name: HashMap<u64, &'static str>,
}

impl<'a> RpcMethodRegistry<'a> {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            name_to_id: HashMap::new(),
            id_to_name: HashMap::new(),
        }
    }

    /// Registers a method by name.
    /// This now accepts arguments as part of the handler.
    pub fn register(&mut self, method_name: &'static str, handler: RpcMethodHandler<'a>) {
        let id = xxh3_64(method_name.as_bytes());
        self.name_to_id.insert(method_name, id);
        self.id_to_name.insert(id, method_name);
        self.handlers.insert(id, handler);
    }

    /// Retrieves a handler based on a method ID (used at dispatch time).
    pub fn get(&mut self, method_id: u64) -> Option<&mut RpcMethodHandler<'a>> {
        self.handlers.get_mut(&method_id)
    }

    /// Optional: reverse-lookup a name from ID.
    pub fn name_for(&self, method_id: u64) -> Option<&'static str> {
        self.id_to_name.get(&method_id).copied()
    }

    /// Optional: forward-lookup an ID from name.
    pub fn id_for(&self, name: &str) -> Option<u64> {
        self.name_to_id.get(name).copied()
    }
}
