use crate::rpc::rpc_internals::RpcMessageType;

/// Represents the header of an RPC (Remote Procedure Call) message.
#[derive(Debug, Clone)]
pub struct RpcHeader {
    /// The type of the RPC message (Call, Response, Event, etc.).
    pub rpc_msg_type: RpcMessageType,

    /// A unique identifier for this RPC message. It helps correlate requests and responses.
    ///
    /// For example:
    /// - For a call, this ID could represent the unique request ID.
    /// - For a response, this ID would match the request ID for which it is responding.
    pub rpc_request_id: u32,

    /// The identifier (or hash) of the method being invoked in this RPC.
    ///
    /// This field helps to identify which method is being called in the remote procedure.
    /// It's often hashed to ensure uniqueness and prevent collisions in method names.
    pub rpc_method_id: u64,

    // TODO: Document; Schemaless metadata
    pub rpc_metadata_bytes: Vec<u8>,
}
