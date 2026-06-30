//! # Application-Defined Constants
//!
//! All constants in this module are **defaults** for the framework. Every real
//! application should define its own values for:
//!
//! - **Method IDs**: Use `rpc_method_id!("my_app.my_method")` to generate
//!   compile-time hashed identifiers. These are scoped to the application
//!   and must be unique within each service boundary.
//! - **Chunk sizes**: Choose based on typical message sizes and latency goals.
//! - **Buffer sizes**: Choose based on expected concurrency and memory budget.
//!
//! The constants provided here are reasonable defaults for getting started.

/// The default maximum chunk size for RPC payloads (64 KB).
///
/// This is an application-defined constant. Each service may choose a different
/// chunk size based on its typical message sizes and latency requirements.
/// Values typically range from 4 KB to 1 MB depending on the use case.
pub const DEFAULT_SERVICE_MAX_CHUNK_SIZE: usize = 1024 * 64;

/// The default buffer size for the MPSC channel used in streaming RPC calls.
///
/// This is an application-defined default and may be overridden per service.
/// The value represents the number of *items* (i.e., `Vec<u8>` chunks) the
/// channel can hold before applying backpressure, not the total size in bytes.
///
/// A small buffer prioritizes low memory usage and responsive backpressure,
/// while a larger buffer can increase throughput by absorbing network jitter
/// at the cost of higher potential memory consumption.
///
/// See `call_rpc_streaming_generic` in the `muxio_rpc_service_caller` crate for usage.
pub const DEFAULT_RPC_STREAM_CHANNEL_BUFFER_SIZE: usize = 8;
