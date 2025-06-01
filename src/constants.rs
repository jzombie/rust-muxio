// Frame related constants
pub const FRAME_LENGTH_FIELD_SIZE: usize = 4;
pub const FRAME_STREAM_ID_OFFSET: usize = 4;
pub const FRAME_SEQ_ID_OFFSET: usize = 8;
pub const FRAME_KIND_OFFSET: usize = 12;
pub const FRAME_TIMESTAMP_OFFSET: usize = 13;
pub const FRAME_HEADER_SIZE: usize = 21;

pub const RPC_FRAME_FRAME_HEADER_SIZE: usize = 16; // Total header size, not including metadata
pub const RPC_FRAME_METADATA_LENGTH_OFFSET: usize = 13; // Metadata length field starts at byte 13
pub const RPC_FRAME_METHOD_ID_OFFSET: usize = 5; // Method ID starts at byte 5 (8 bytes long)
pub const RPC_FRAME_ID_OFFSET: usize = 1; // ID starts at byte 1 (4 bytes long)
pub const RPC_FRAME_MSG_TYPE_OFFSET: usize = 0; // Message type starts at byte 0 (1 byte long)
pub const RPC_FRAME_METHOD_ID_SIZE: usize = 8; // Size of method_id (u64) in bytes
pub const RPC_FRAME_METADATA_LENGTH_SIZE: usize = 2; // Size of metadata length field (u16)
