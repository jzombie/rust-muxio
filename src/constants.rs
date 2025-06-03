// Frame related constants
pub const FRAME_LENGTH_FIELD_SIZE: usize = 4;
pub const FRAME_STREAM_ID_OFFSET: usize = 4;
pub const FRAME_SEQ_ID_OFFSET: usize = 8;
pub const FRAME_KIND_OFFSET: usize = 12;
pub const FRAME_TIMESTAMP_OFFSET: usize = 13;
pub const FRAME_HEADER_SIZE: usize = 21;

/// Byte offset where the metadata length field begins.
/// This field is a fixed-size 2-byte unsigned integer (u16)
/// indicating the length in bytes of the metadata section.
pub const RPC_FRAME_METADATA_LENGTH_OFFSET: usize = 13;

/// Byte offset where the 8-byte method ID (u64) begins.
/// This represents the hashed method identifier used in routing.
pub const RPC_FRAME_METHOD_ID_OFFSET: usize = 5;

/// Byte offset where the 4-byte RPC ID (u32) begins.
/// This is the unique request/response correlation ID.
pub const RPC_FRAME_ID_OFFSET: usize = 1;

/// Byte offset of the 1-byte message type (u8).
/// Values correspond to enum `RpcMessageType` variants.
pub const RPC_FRAME_MSG_TYPE_OFFSET: usize = 0;

/// Size in bytes of the method ID field (u64).
pub const RPC_FRAME_METHOD_ID_SIZE: usize = 8;

/// Size in bytes of the metadata length field (u16).
/// This field tells how many bytes of metadata follow the header.
pub const RPC_FRAME_METADATA_LENGTH_SIZE: usize = 2;

/// Total size of the fixed-length header prefix before metadata.
/// Computed as: offset of metadata length field + its size.
/// Does not include metadata or payload data.
pub const RPC_FRAME_FRAME_HEADER_SIZE: usize =
    RPC_FRAME_METADATA_LENGTH_OFFSET + RPC_FRAME_METADATA_LENGTH_SIZE; // 13 + 2 = 15
