use crate::rpc::rpc_internals::RpcHeader;

// TODO: Prefix all with `rpc`
/// Represents an outbound RPC call request.
///
/// An `RpcRequest` is initiated by a client and contains the encoded
/// parameters for a specific remote method. Optionally, it may include a
/// buffered payload and a flag indicating whether the full message is
/// ready to send immediately (finalized).
#[derive(PartialEq, Debug)]
pub struct RpcRequest {
    /// Unique identifier for the remote method to invoke.
    ///
    /// This can be a sequential or a hashed ID (e.g., XXH3 or FNV) that maps
    /// to a known method on the remote server.
    pub rpc_method_id: u64,

    /// Optional encoded metadata (typically parameters).
    ///
    /// These are serialized function arguments and are transmitted in the
    /// `RpcHeader.metadata_bytes` field. If `None`, the metadata section is empty.
    pub rpc_param_bytes: Option<Vec<u8>>,

    /// Optional payload that should be sent immediately after the header.
    ///
    /// This is useful for single-frame RPCs where the entire message (metadata
    /// and payload) is known up front. If provided, it is sent during `call()`.
    pub rpc_prebuffered_payload_bytes: Option<Vec<u8>>,

    /// Indicates whether the request is fully formed and no more payload is expected.
    ///
    /// If true, the stream is closed immediately after sending the header and payload.
    pub is_finalized: bool,
}

/// Represents a response to a prior RPC request.
///
/// The `RpcResponse` contains the original request ID (header ID), the method ID,
/// and any result status or response payload. It may be used to encode a response
/// frame or passed to the `respond()` function to emit a full RPC reply.
#[derive(PartialEq, Debug)]
pub struct RpcResponse {
    /// The request header ID this response corresponds to.
    ///
    /// This *must match* the `RpcHeader.rpc_request_id` from the initiating request.
    pub rpc_request_id: u32,

    /// The method ID associated with this response.
    ///
    /// This should match the original `method_id` from the request.
    ///
    /// Note: While the internal routing mechanism relies on the header ID to correlate
    /// responses, user-defined services may still benefit from retaining the method ID to
    /// dispatch or verify responses against known handlers.
    pub rpc_method_id: u64,

    /// Optional result status byte (e.g., success/fail/system error).
    ///
    /// If present, this value is embedded in the response metadata and typically
    /// represents a single-byte status code.
    ///
    /// Note: Muxio's core library does not enforce any meaning behind any result status,
    /// though by convention, 0 represents success.
    pub rpc_result_status: Option<u8>,

    /// Optional payload to return with the response.
    ///
    /// If set, this will be sent immediately as the response payload.
    pub prebuffered_payload_bytes: Option<Vec<u8>>,

    /// Marks whether the response stream is complete.
    ///
    /// If true, the stream will be ended immediately after sending the header
    /// and payload. If false, the caller is expected to stream additional data and manually
    /// end the stream.
    pub is_finalized: bool,
}

impl RpcResponse {
    /// Constructs an `RpcResponse` from a received `RpcHeader`.
    ///
    /// This is typically called on the server side when processing a new request.
    /// The metadata is interpreted as a single-byte result status if present.
    ///
    /// # Arguments
    /// - `rpc_header`: The header received from the client.
    ///
    /// # Returns
    /// A new `RpcResponse` with the same request ID and method ID, and
    /// optionally a result status if metadata exists.
    pub fn from_rpc_header(rpc_header: &RpcHeader) -> RpcResponse {
        RpcResponse {
            rpc_request_id: rpc_header.rpc_request_id,
            rpc_method_id: rpc_header.rpc_method_id,
            rpc_result_status: {
                match rpc_header.rpc_metadata_bytes.len() {
                    0 => None,
                    _ => Some(rpc_header.rpc_metadata_bytes[0]),
                }
            },
            prebuffered_payload_bytes: None,
            is_finalized: false, // Hardcoded to false because it is currently non-determinable from the header alone
        }
    }
}
