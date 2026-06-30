use super::endpoint_utils::process_single_prebuffered_request;
use super::{
    endpoint::StreamResponder,
    error::RpcServiceEndpointError,
    with_handlers_trait::{WithHandlers, WithStreamHandlers},
};
use futures::future::join_all;
use muxio_core::rpc::{
    RpcDispatcher,
    rpc_internals::{RpcStreamEvent, rpc_trait::RpcEmit},
};
use muxio_rpc_service::constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE;
use std::sync::Mutex as StdMutex;
use std::{collections::hash_map::Entry, future::Future, marker::Send, sync::Arc};

#[async_trait::async_trait]
pub trait RpcServiceEndpointInterface<C>: Send + Sync
where
    C: Send + Sync + Clone + 'static,
{
    type HandlersLock: WithHandlers<C> + 'static;
    type StreamHandlersLock: WithStreamHandlers<C> + 'static;

    fn get_prebuffered_handlers(&self) -> Arc<Self::HandlersLock>;

    fn get_stream_handlers(&self) -> Arc<Self::StreamHandlersLock>;

    /// Registers a new pre-buffered RPC method handler with this endpoint.
    ///
    /// Pre-buffered methods are those where the entire request payload is
    /// received and buffered before the handler is invoked. The handler
    /// then processes the request and returns a single, complete response payload.
    ///
    /// # Arguments
    /// * `method_id` - A unique identifier for the RPC method. This should typically
    ///                 be generated using the `rpc_method_id!` macro.
    /// * `handler` - An asynchronous closure that will be executed when a request
    ///               for `method_id` is received. It takes the connection context `C`
    ///               and the raw request bytes (`Vec<u8>`), and must return a `Result`
    ///               containing the response bytes (`Vec<u8>`) on success, or a boxed
    ///               `std::error::Error` on failure.
    ///
    /// # Errors
    /// Returns an `RpcServiceEndpointError` if a handler for the given `method_id`
    /// is already registered.
    async fn register_prebuffered<F, Fut>(
        &self,
        method_id: u64,
        handler: F,
    ) -> Result<(), RpcServiceEndpointError>
    where
        F: Fn(Vec<u8>, C) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>
            + Send
            + 'static,
    {
        // Also check streaming handlers to prevent ID clashes
        let stream_exists = self
            .get_stream_handlers()
            .with_stream_handlers(|handlers| handlers.contains_key(&method_id))
            .await;
        if stream_exists {
            return Err(RpcServiceEndpointError::Handler(
                format!("A streaming handler for method ID {method_id} is already registered.")
                    .into(),
            ));
        }

        self.get_prebuffered_handlers()
            .with_handlers(|handlers| match handlers.entry(method_id) {
                // `method_id` is now u64, matches HashMap key
                Entry::Occupied(_) => {
                    let err_msg =
                        format!("A handler for method ID {method_id} is already registered.");
                    Err(RpcServiceEndpointError::Handler(err_msg.into()))
                }
                Entry::Vacant(entry) => {
                    let wrapped = move |request_bytes: Vec<u8>, ctx: C| {
                        Box::pin(handler(request_bytes, ctx))
                            as std::pin::Pin<Box<dyn Future<Output = _> + Send>>
                    };
                    entry.insert(Arc::new(wrapped));
                    Ok(())
                }
            })
            .await
    }

    /// Registers a new streaming RPC method handler with this endpoint.
    ///
    /// Streaming methods receive individual [`RpcStreamEvent`]s as they arrive
    /// from the transport (rather than being buffered into a single payload).
    /// The handler is called synchronously for each event — `Header`,
    /// `PayloadChunk`, `End`, and `Error` — and can use the provided
    /// [`StreamResponder`] to send properly framed response chunks back
    /// to the caller.
    ///
    /// # Arguments
    /// * `method_id` - A unique identifier for the RPC method.
    /// * `handler` - A closure invoked synchronously for each stream event.
    ///   Receives the event, a [`StreamResponder`] for sending response chunks
    ///   back, and the connection context.
    ///
    /// # Errors
    /// Returns an `RpcServiceEndpointError` if a handler for `method_id` is
    /// already registered (either prebuffered or streaming).
    async fn register_stream_handler<H>(
        &self,
        method_id: u64,
        handler: H,
    ) -> Result<(), RpcServiceEndpointError>
    where
        H: Fn(RpcStreamEvent, StreamResponder, C) + Send + Sync + 'static,
    {
        // Also check prebuffered handlers to prevent ID clashes
        let prebuffered_exists = self
            .get_prebuffered_handlers()
            .with_handlers(|handlers| handlers.contains_key(&method_id))
            .await;
        if prebuffered_exists {
            return Err(RpcServiceEndpointError::Handler(
                format!("A prebuffered handler for method ID {method_id} is already registered.")
                    .into(),
            ));
        }

        self.get_stream_handlers()
            .with_stream_handlers(|handlers| match handlers.entry(method_id) {
                Entry::Occupied(_) => {
                    let err_msg =
                        format!("A streaming handler for method ID {method_id} is already registered.");
                    Err(RpcServiceEndpointError::Handler(err_msg.into()))
                }
                Entry::Vacant(entry) => {
                    entry.insert(Arc::new(handler));
                    Ok(())
                }
            })
            .await
    }

    /// Reads raw bytes from the transport, decodes them into RPC requests,
    /// invokes the appropriate handler, and sends back a response.
    ///
    /// For streaming methods (registered via [`register_stream_handler`]),
    /// events are forwarded to the streaming handler as they arrive rather
    /// than being accumulated. For prebuffered methods, the entire request
    /// is accumulated and then passed to the handler.
    async fn read_bytes<'a, E>(
        &self,
        dispatcher: &mut RpcDispatcher<'a>,
        context: C,
        bytes: &[u8],
        on_emit: E,
    ) -> Result<(), RpcServiceEndpointError>
    where
        E: RpcEmit + Send + Sync + Clone + 'static,
    {
        // --- Streaming Method Routing ---
        // Take a snapshot of registered streaming handlers. If any exist,
        // install a router on the dispatcher so that incoming Header events
        // for streaming methods get per-request handlers installed (bypassing
        // the prebuffered catch-all accumulator).
        //
        // Pending responders are collected during routing so that after
        // read_bytes we can install encoder-backed writers on them.
        let pending_responders: Arc<StdMutex<Vec<StreamResponder>>> =
            Arc::new(StdMutex::new(Vec::new()));

        let stream_handlers = self.get_stream_handlers();
        let has_stream_handlers = stream_handlers
            .with_stream_handlers(|handlers| !handlers.is_empty())
            .await;

        if has_stream_handlers {
            let handlers_snapshot = stream_handlers
                .with_stream_handlers(|handlers| handlers.clone())
                .await;
            let ctx_clone = context.clone();
            let responders = pending_responders.clone();

            dispatcher.set_stream_method_router(move |method_id, request_id| {
                handlers_snapshot.get(&method_id).map(|handler| {
                    let h = Arc::clone(handler);
                    let ctx = ctx_clone.clone();
                    let respond = StreamResponder::new(request_id);
                    responders.lock().expect("Pending responders lock poisoned").push(respond.clone());
                    let boxed: Box<dyn FnMut(RpcStreamEvent) + Send + 'a> =
                        Box::new(move |event: RpcStreamEvent| {
                            h(event, respond.clone(), ctx.clone());
                        });
                    boxed
                })
            });
        }

        // --- Stage 1: Decode Incoming Frames & Identify Finalized Requests ---
        // This synchronously processes the raw byte stream received from the transport.
        // It updates the dispatcher's internal state to reflect ongoing and completed requests.
        // It then collects all requests that are now fully received and ready for handling.
        let request_ids = dispatcher.read_bytes(bytes)?;

        // After decoding, install encoder-backed writers on all pending
        // responders so that future respond() calls go directly to the
        // transport.  Any writes that happened during read_bytes (before
        // the writer was set) were buffered inside StreamResponder and
        // are flushed now.
        {
            let mut responders = pending_responders.lock().expect("Pending responders lock poisoned");
            for respond in responders.iter() {
                if let Ok(writer) = dispatcher.create_response_writer(
                    respond.request_id,
                    DEFAULT_SERVICE_MAX_CHUNK_SIZE,
                    on_emit.clone(),
                ) {
                    respond.set_writer(writer);
                }
            }
            responders.clear();
        }

        // Collect finalized requests for prebuffered handler dispatch.
        let mut finalized_requests = Vec::new();
        for id in request_ids {
            // Check if the request associated with this ID is complete.
            if dispatcher.is_rpc_request_finalized(id).unwrap_or(false) {
                // If complete, extract the full request data from the dispatcher.
                if let Some(req) = dispatcher.delete_rpc_request(id) {
                    finalized_requests.push((id, req));
                }
            }
        }

        // If no finalized requests were found in the incoming bytes, there's nothing more to do.
        if finalized_requests.is_empty() {
            return Ok(());
        }

        // --- Stage 2: Asynchronously Execute RPC Handlers ---
        // This stage dispatches each identified request to its corresponding,
        // user-defined asynchronous handler. Handlers perform the application-specific
        // logic and generate the raw response payload.
        // This stage runs concurrently for all requests that arrived,
        // without blocking the main event loop.
        let handlers_arc = self.get_prebuffered_handlers();
        let mut response_futures = Vec::new();
        for (request_id, request) in finalized_requests {
            // `request_id` is u32 here
            let handlers_arc_clone = handlers_arc.clone();
            let context_clone = context.clone();

            // Create an async task (future) for processing this single request.
            // This future will look up the handler, execute it, and format the response.
            let future = process_single_prebuffered_request(
                handlers_arc_clone,
                context_clone,
                request_id, // This is u32
                request,
            );
            response_futures.push(future);
        }

        // Await the completion of all handler futures. This pauses `read_bytes`
        // until all responses are ready, but allows other tasks on the executor to run.
        let responses = join_all(response_futures).await;

        // --- Stage 3: Synchronously Encode & Emit Responses ---
        // This stage takes the application-level responses generated by the handlers,
        // encodes them into the RPC protocol format, and emits them back onto the transport.
        // This is a synchronous operation that updates the dispatcher's state
        // and sends out the final byte chunks.
        for response in responses {
            let _ = dispatcher.respond(response, DEFAULT_SERVICE_MAX_CHUNK_SIZE, on_emit.clone());
        }

        Ok(())
    }
}
