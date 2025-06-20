pub const DEFAULT_SERVICE_MAX_CHUNK_SIZE: usize = 1024 * 64;
// pub const DEFAULT_SERVICE_MAX_CHUNK_SIZE: usize = 1024;

/// The default buffer size for the MPSC channel used in streaming RPC calls.
///
/// This value represents the number of *items* (i.e., `Vec<u8>` chunks) the
/// channel can hold before applying backpressure, not the total size in bytes.
///
/// A small buffer prioritizes low memory usage and responsive backpressure,
/// while a larger buffer can increase throughput by absorbing network jitter
/// at the cost of higher potential memory consumption.
///
/// See `call_rpc_streaming_generic` in the `muxio_rpc_service_caller` crate for usage.
pub const DEFAULT_RPC_STREAM_CHANNEL_BUFFER_SIZE: usize = 8;
