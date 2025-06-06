use crate::frame::{FrameDecodeError, FrameEncodeError};
use crate::rpc::{
    RpcRequest, RpcResponse,
    rpc_internals::{
        RpcEmit, RpcHeader, RpcMessageType, RpcRespondableSession, RpcResponseHandler,
        RpcStreamEncoder, RpcStreamEvent,
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
    rpc_session: RpcRespondableSession<'a>,

    // TODO: Document how this must be unique per session
    /// Monotonic ID generator for outbound RPC request headers.
    next_header_id: u32,

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
        let rpc_session = RpcRespondableSession::new();

        let mut instance = Self {
            rpc_session,
            next_header_id: increment_u32_id(),
            rpc_request_queue: Arc::new(Mutex::new(VecDeque::new())),
        };

        instance.init_catch_all_response_handler();

        instance
    }

    /// Internal helper to register a global response event handler.
    ///
    /// This callback listens for all response stream events, and tracks them
    /// by `rpc_header_id` in the internal `rpc_request_queue`.
    ///
    /// - Adds new requests on `Header`
    /// - Appends payload chunks on `PayloadChunk`
    /// - Marks request as complete on `End`
    ///
    /// This design enables response stream tracking even when no specific
    /// handler is registered per-request.
    fn init_catch_all_response_handler(&mut self) {
        let rpc_request_queue_ref = Arc::clone(&self.rpc_request_queue);

        self.rpc_session
            .set_catch_all_response_handler(Box::new(move |event: RpcStreamEvent| {
                // TODO: Don't use unwrap
                let mut queue = rpc_request_queue_ref.lock().unwrap();

                // TODO: Delete from queue if request is canceled mid-flight
                match event {
                    RpcStreamEvent::Header {
                        rpc_header_id,
                        rpc_header,
                        ..
                    } => {
                        // Convert metadata to parameter bytes
                        let param_bytes = match rpc_header.metadata_bytes.len() {
                            0 => None,
                            _ => Some(rpc_header.metadata_bytes),
                        };

                        let rpc_request = RpcRequest {
                            method_id: rpc_header.method_id,
                            param_bytes,
                            pre_buffered_payload_bytes: None, // No payload yet
                            is_finalized: false,
                        };

                        queue.push_back((rpc_header_id, rpc_request));
                    }

                    RpcStreamEvent::PayloadChunk {
                        rpc_header_id,
                        bytes,
                        ..
                    } => {
                        // Look for the existing RpcRequest in the queue and borrow it mutably
                        if let Some((_, rpc_request)) =
                            queue.iter_mut().find(|(id, _)| *id == rpc_header_id)
                        {
                            // Append bytes to the payload
                            let payload = rpc_request
                                .pre_buffered_payload_bytes
                                .get_or_insert_with(Vec::new);
                            payload.extend_from_slice(&bytes);
                        }
                    }

                    RpcStreamEvent::End { rpc_header_id, .. } => {
                        // Finalize and process the full message when the stream ends
                        if let Some((_, rpc_request)) =
                            queue.iter_mut().find(|(id, _)| *id == rpc_header_id)
                        {
                            // Set the `is_finalized` flag to true when the stream ends
                            rpc_request.is_finalized = true;
                        }
                    }

                    RpcStreamEvent::Error {
                        rpc_header_id,
                        rpc_method_id,
                        frame_decode_error,
                    } => {
                        // TODO: Handle errors
                        println!(
                            "Error in stream. Method: {:?} {:?}: {:?}",
                            rpc_method_id, rpc_header_id, frame_decode_error
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
    /// - `pre_buffer_response`: If true, buffer all chunks into one event
    pub fn call<E, R>(
        &mut self,
        rpc_request: RpcRequest,
        max_chunk_size: usize,
        on_emit: E,
        on_response: Option<R>,
        pre_buffer_response: bool,
    ) -> Result<RpcStreamEncoder<E>, FrameEncodeError>
    where
        E: RpcEmit,
        R: RpcResponseHandler + 'a,
    {
        let method_id = rpc_request.method_id;

        let header_id: u32 = self.next_header_id;
        self.next_header_id = increment_u32_id();

        // Convert parameter bytes to metadata
        let metadata_bytes = match rpc_request.param_bytes {
            Some(param_bytes) => param_bytes,
            None => vec![],
        };

        let request_header = RpcHeader {
            msg_type: RpcMessageType::Call,
            id: header_id,
            method_id,
            metadata_bytes,
        };

        // Directly pass the closure as `on_emit` without borrowing it
        let mut encoder = self.rpc_session.init_respondable_request(
            request_header,
            max_chunk_size,
            on_emit,
            on_response,
            pre_buffer_response,
        )?;

        // If the RPC request has a buffered payload, send it here
        if let Some(pre_buffered_payload_bytes) = rpc_request.pre_buffered_payload_bytes {
            encoder.push_bytes(&pre_buffered_payload_bytes)?;
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
    /// The `RpcResponse` must match a valid request by `request_header_id`.
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
            id: rpc_response.request_header_id,
            msg_type: RpcMessageType::Response,
            method_id: rpc_response.method_id,
            metadata_bytes: {
                match rpc_response.result_status {
                    Some(result_status) => vec![result_status],
                    None => vec![],
                }
            },
        };

        let mut response_encoder =
            self.rpc_session
                .start_reply_stream(rpc_response_header, max_chunk_size, on_emit)?;

        if let Some(pre_buffered_payload_bytes) = rpc_response.pre_buffered_payload_bytes {
            response_encoder.push_bytes(&pre_buffered_payload_bytes)?;
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
    pub fn receive_bytes(&mut self, bytes: &[u8]) -> Result<Vec<u32>, FrameDecodeError> {
        // Process the incoming bytes
        self.rpc_session.receive_bytes(bytes)?;

        // List of request header IDs which are currently in progress
        let queue = self
            .rpc_request_queue
            .lock()
            .map_err(|_| FrameDecodeError::CorruptFrame)?;
        let active_request_header_ids: Vec<u32> =
            queue.iter().map(|(header_id, _)| *header_id).collect();

        Ok(active_request_header_ids)
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
