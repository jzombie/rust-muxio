use crate::frame::{FrameDecodeError, FrameEncodeError};
use crate::rpc::{
    RpcRequest, RpcResponse,
    rpc_internals::{
        RpcHeader, RpcMessageType, RpcRespondableSession, RpcStreamEncoder, RpcStreamEvent,
        rpc_trait::{RpcEmit, RpcResponseHandler},
    },
};
use crate::utils::increment_u32_id;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Manages RPC request dispatching and response handling over a framed transport.
///
/// `RpcDispatcher` serves as a runtime coordinator for encoding outbound
/// RPC requests, emitting them as framed streams, and handling inbound
/// RPC responses via a shared request queue.
///
/// This dispatcher operates in a **non-async** model using **layered callbacks**.
/// It is compatible with **WASM** and **multithreaded runtimes**, and supports:
/// - Chunked streaming of request/response payloads
/// - Request correlation via `header_id`
/// - Mid-stream cancellation
///
/// Internally, it wraps a `RpcRespondableSession` and maintains a synchronized
/// request queue for tracking inbound response metadata and payloads.
pub struct RpcDispatcher<'a> {
    /// Core session responsible for managing stream lifecycles and handlers.
    rpc_respondable_session: RpcRespondableSession<'a>,

    // TODO: Document how this must be unique per session
    /// Monotonic ID generator for outbound RPC request headers.
    next_rpc_request_id: u32,

    /// Queue of currently active inbound responses from remote peers.
    ///
    /// Each entry is `(header_id, RpcRequest)` and is updated via stream
    /// events triggered by the session handler. This queue is shared to
    /// allow external inspection or draining of finalized responses.
    rpc_request_queue: Arc<Mutex<VecDeque<(u32, RpcRequest)>>>,
}

impl<'a> RpcDispatcher<'a> {
    /// Constructs a new `RpcDispatcher`, initializes the internal session,
    /// and installs a default response handler that populates the request queue.
    ///
    /// The handler collects incoming response stream events and maintains
    /// them in a shared, thread-safe queue for downstream access.
    pub fn new() -> Self {
        let rpc_respondable_session = RpcRespondableSession::new();

        let mut instance = Self {
            rpc_respondable_session,
            next_rpc_request_id: increment_u32_id(),
            rpc_request_queue: Arc::new(Mutex::new(VecDeque::new())),
        };

        instance.init_catch_all_response_handler();

        instance
    }

    /// Internal helper to register a global response event handler.
    ///
    /// This callback listens for all incoming response stream events and updates
    /// the internal `rpc_request_queue` by `rpc_request_id`. It is installed as a
    /// global fallback handler to track all incoming responses, even those without
    /// a dedicated handler.
    ///
    /// Behavior:
    /// - On `Header`: pushes a new `RpcRequest` into the queue.
    /// - On `PayloadChunk`: appends bytes to the matching request’s payload.
    /// - On `End`: marks the request as finalized.
    ///
    /// ## Thread Safety and Poisoning
    ///
    /// The queue is protected by a `Mutex`. If this mutex has been poisoned
    /// (i.e., a previous thread panicked while holding the lock), this method
    /// will `panic!` immediately. This is a deliberate design choice:
    ///
    /// - A poisoned queue likely means shared state is inconsistent or partially mutated.
    /// - Continuing execution in this situation could lead to data loss, incorrect routing,
    ///   or undefined behavior.
    /// - Crashing fast provides better safety and debugging signals in production or test.
    ///
    /// If graceful recovery is ever desired, this behavior should be restructured
    /// behind a configurable panic policy or error reporting mechanism.
    fn init_catch_all_response_handler(&mut self) {
        let rpc_request_queue_ref = Arc::clone(&self.rpc_request_queue);

        self.rpc_respondable_session
            .set_catch_all_response_handler(Box::new(move |event: RpcStreamEvent| {
                let Ok(mut queue) = rpc_request_queue_ref.lock() else {
                    // If the lock is poisoned, it likely means another thread panicked while
                    // holding the mutex. The internal state of the request queue may now be
                    // inconsistent or partially mutated.
                    //
                    // Continuing execution could result in incorrect dispatch behavior,
                    // undefined state transitions, or silent data loss.
                    //
                    // This should be treated as a critical failure and escalated appropriately.
                    panic!(
                        "[RpcDispatcher] Critical: Request queue mutex poisoned. \
                        Dispatcher may be in an inconsistent state. \
                        Event dropped to avoid undefined behavior."
                    );
                };

                // TODO: Delete from queue if request is canceled mid-flight
                match event {
                    RpcStreamEvent::Header {
                        rpc_request_id,
                        rpc_header,
                        ..
                    } => {
                        // Convert metadata to parameter bytes
                        let rpc_param_bytes = match rpc_header.rpc_metadata_bytes.len() {
                            0 => None,
                            _ => Some(rpc_header.rpc_metadata_bytes),
                        };

                        let rpc_request = RpcRequest {
                            rpc_method_id: rpc_header.rpc_method_id,
                            rpc_param_bytes,
                            rpc_prebuffered_payload_bytes: None, // No payload yet
                            is_finalized: false,
                        };

                        queue.push_back((rpc_request_id, rpc_request));
                    }

                    RpcStreamEvent::PayloadChunk {
                        rpc_request_id,
                        bytes,
                        ..
                    } => {
                        // Look for the existing RpcRequest in the queue and borrow it mutably
                        if let Some((_, rpc_request)) =
                            queue.iter_mut().find(|(id, _)| *id == rpc_request_id)
                        {
                            // Append bytes to the payload
                            let payload = rpc_request
                                .rpc_prebuffered_payload_bytes
                                .get_or_insert_with(Vec::new);
                            payload.extend_from_slice(&bytes);
                        }
                    }

                    RpcStreamEvent::End { rpc_request_id, .. } => {
                        // Finalize and process the full message when the stream ends
                        if let Some((_, rpc_request)) =
                            queue.iter_mut().find(|(id, _)| *id == rpc_request_id)
                        {
                            // Set the `is_finalized` flag to true when the stream ends
                            rpc_request.is_finalized = true;
                        }
                    }

                    RpcStreamEvent::Error {
                        rpc_request_id,
                        rpc_method_id,
                        frame_decode_error,
                    } => {
                        // TODO: Handle errors
                        println!(
                            "Error in stream. Method: {:?} {:?}: {:?}",
                            rpc_method_id, rpc_request_id, frame_decode_error
                        );
                    }
                }
            }));
    }

    /// Initiates an outbound RPC `Call` with a structured `RpcRequest`.
    ///
    /// The method constructs a `RpcHeader`, serializes its metadata,
    /// and begins the transmission via a `RpcStreamEncoder`.
    ///
    /// - If a payload is provided, it is sent immediately after the header.
    /// - If `is_finalized` is set, the stream is ended immediately.
    /// - A response handler can optionally be registered for correlated responses.
    ///
    /// # Arguments
    /// - `rpc_request`: Encoded request with method ID and metadata
    /// - `max_chunk_size`: Max frame size before splitting into multiple frames
    /// - `on_emit`: Callback to transmit the encoded frames
    /// - `on_response`: Optional response stream handler
    /// - `prebuffer_response`: If true, buffer all chunks into one event
    pub fn call<E, R>(
        &mut self,
        rpc_request: RpcRequest,
        max_chunk_size: usize,
        on_emit: E,
        on_response: Option<R>,
        prebuffer_response: bool,
    ) -> Result<RpcStreamEncoder<E>, FrameEncodeError>
    where
        E: RpcEmit,
        R: RpcResponseHandler + 'a,
    {
        let rpc_method_id = rpc_request.rpc_method_id;

        let rpc_request_id: u32 = self.next_rpc_request_id;
        self.next_rpc_request_id = increment_u32_id();

        // Convert parameter bytes to metadata
        let rpc_metadata_bytes = match rpc_request.rpc_param_bytes {
            Some(param_bytes) => param_bytes,
            None => vec![],
        };

        let request_header = RpcHeader {
            rpc_msg_type: RpcMessageType::Call,
            rpc_request_id,
            rpc_method_id,
            rpc_metadata_bytes,
        };

        // Directly pass the closure as `on_emit` without borrowing it
        let mut encoder = self.rpc_respondable_session.init_respondable_request(
            request_header,
            max_chunk_size,
            on_emit,
            on_response,
            prebuffer_response,
        )?;

        // If the RPC request has a buffered payload, send it here
        if let Some(prebuffered_payload_bytes) = rpc_request.rpc_prebuffered_payload_bytes {
            encoder.write_bytes(&prebuffered_payload_bytes)?;
        }

        // If the RPC request is pre-finalized, close the stream
        if rpc_request.is_finalized {
            encoder.flush()?;
            encoder.end_stream()?;
        }

        Ok(encoder)
    }

    /// Constructs and sends a response stream for a previously received request.
    ///
    /// The `RpcResponse` must match a valid request by `request_id`.
    /// Optionally, it may include a result status byte or payload. If
    /// `is_finalized` is true, the stream is immediately ended.
    ///
    /// # Arguments
    /// - `rpc_response`: Structured reply referencing a known request
    /// - `max_chunk_size`: Chunking threshold for large response payloads
    /// - `on_emit`: Callback to transmit the encoded response stream
    pub fn respond<E>(
        &mut self,
        rpc_response: RpcResponse,
        max_chunk_size: usize,
        on_emit: E,
    ) -> Result<RpcStreamEncoder<E>, FrameEncodeError>
    where
        E: RpcEmit,
    {
        let rpc_response_header = RpcHeader {
            rpc_request_id: rpc_response.rpc_request_id,
            rpc_msg_type: RpcMessageType::Response,
            rpc_method_id: rpc_response.rpc_method_id,
            // TODO: Be sure to document how this works (on responses, the only metadata sent
            // is the result status or nothing at all)
            rpc_metadata_bytes: {
                match rpc_response.rpc_result_status {
                    Some(rpc_result_status) => vec![rpc_result_status],
                    None => vec![],
                }
            },
        };

        let mut response_encoder = self.rpc_respondable_session.start_reply_stream(
            rpc_response_header,
            max_chunk_size,
            on_emit,
        )?;

        if let Some(rpc_prebuffered_payload_bytes) = rpc_response.rpc_prebuffered_payload_bytes {
            response_encoder.write_bytes(&rpc_prebuffered_payload_bytes)?;
        }

        if rpc_response.is_finalized {
            response_encoder.flush()?;
            response_encoder.end_stream()?;
        }

        Ok(response_encoder)
    }

    /// Feeds incoming byte stream data into the `RpcRespondableSession` for
    /// decoding and stream reassembly. This method should be called whenever
    /// new bytes are received from the transport layer (e.g. socket).
    ///
    /// Internally, this triggers stream event callbacks—such as header,
    /// payload, and end events—that populate and update the internal
    /// `rpc_request_queue` used for tracking in-flight requests.
    ///
    /// # Returns
    ///
    /// A `Vec<u32>` containing the active request `header_id`s currently
    /// stored in the shared `rpc_request_queue`. These represent all requests
    /// that:
    /// - Have received at least a `Header` frame, and
    /// - Have not yet been deleted or fully handled.
    ///
    /// This list is useful for downstream logic to inspect which requests are
    /// pending, in-progress, or ready for consumption.
    ///
    /// # Errors
    ///
    /// - Returns `FrameDecodeError` if the incoming frame is invalid or if
    ///   the `rpc_request_queue` mutex is poisoned.
    pub fn read_bytes(&mut self, bytes: &[u8]) -> Result<Vec<u32>, FrameDecodeError> {
        // Process the incoming bytes
        self.rpc_respondable_session.read_bytes(bytes)?;

        // List of request header IDs which are currently in progress
        let queue = self
            .rpc_request_queue
            .lock()
            .map_err(|_| FrameDecodeError::CorruptFrame)?;
        let active_request_ids: Vec<u32> = queue.iter().map(|(header_id, _)| *header_id).collect();

        Ok(active_request_ids)
    }

    /// Attempts to retrieve a lock on the request queue if the given
    /// `header_id` exists in it.
    ///
    /// Returns the full `MutexGuard` to the queue, not just the matched entry.
    /// Caller is responsible for re-checking the queue contents under the guard.
    pub fn get_rpc_request(
        &self,
        header_id: u32,
    ) -> Option<std::sync::MutexGuard<'_, VecDeque<(u32, RpcRequest)>>> {
        let queue = self.rpc_request_queue.lock().ok()?;

        // You can't return a reference to an element inside a MutexGuard
        // so we return the full guard instead
        if queue.iter().any(|(id, _)| *id == header_id) {
            Some(queue) // Caller must search again within the guard
        } else {
            None
        }
    }

    /// Returns `true` if the request with `header_id` has received a
    /// complete stream (`End` event observed), or `None` if it is missing.
    pub fn is_rpc_request_finalized(&self, header_id: u32) -> Option<bool> {
        let queue = self.rpc_request_queue.lock().ok()?;
        queue
            .iter()
            .find(|(id, _)| *id == header_id)
            .map(|(_, req)| req.is_finalized)
    }

    /// Removes the request with `header_id` from the internal queue and
    /// transfers ownership of the `RpcRequest` to the caller.
    ///
    /// Returns `None` if the request was not found.
    pub fn delete_rpc_request(&self, header_id: u32) -> Option<RpcRequest> {
        let mut queue = self.rpc_request_queue.lock().ok()?;

        if let Some(index) = queue.iter().position(|(id, _)| *id == header_id) {
            // Remove and return just the RpcRequest
            Some(queue.remove(index)?.1)
        } else {
            None
        }
    }
}
