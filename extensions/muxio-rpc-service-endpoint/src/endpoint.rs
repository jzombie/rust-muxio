use super::RpcServiceEndpointInterface;
use muxio_core::rpc::rpc_internals::RpcStreamEvent;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::{future::Future, marker::PhantomData, pin::Pin, sync::Arc};

// --- Conditionally Alias the Mutex Implementation ---
#[cfg(not(feature = "tokio_support"))]
use std::sync::Mutex;
#[cfg(feature = "tokio_support")]
use tokio::sync::Mutex;

// --- Generic Definitions ---
pub type RpcPrebufferedHandler<C> = Arc<
    dyn Fn(
            Vec<u8>,
            C,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

/// Handle for sending properly framed response chunks back to the caller
/// from within a streaming handler.  Calls to [`respond`] are buffered and
/// flushed through the dispatcher after each `read_bytes` cycle.
#[derive(Clone)]
pub struct StreamResponder {
    request_id: u32,
    buffer: Arc<StdMutex<Vec<PendingResponse>>>,
}

#[derive(Clone)]
pub(crate) struct PendingResponse {
    pub request_id: u32,
    pub chunk: Vec<u8>,
    pub is_finalized: bool,
}

impl StreamResponder {
    pub(crate) fn new(request_id: u32, buffer: Arc<StdMutex<Vec<PendingResponse>>>) -> Self {
        Self { request_id, buffer }
    }

    /// Queue a response chunk.  Call with `is_finalized: true` on the
    /// last chunk to signal the end of the response stream.
    pub fn respond(&self, chunk: Vec<u8>, is_finalized: bool) {
        self.buffer.lock().unwrap().push(PendingResponse {
            request_id: self.request_id,
            chunk,
            is_finalized,
        });
    }
}

/// A streaming RPC handler receives individual [`RpcStreamEvent`]s as they
/// arrive from the transport, along with a [`StreamResponder`] for sending
/// properly framed response chunks back to the caller.
pub type RpcStreamHandler<C> = Arc<
    dyn Fn(RpcStreamEvent, StreamResponder, C) + Send + Sync,
>;

/// A concrete RPC service endpoint, generic over a context type `C`.
///
/// Supports both prebuffered (request/response) and streaming RPC handlers.
pub struct RpcServiceEndpoint<C>
where
    C: Send + Sync + Clone + 'static,
{
    prebuffered_handlers: Arc<Mutex<HashMap<u64, RpcPrebufferedHandler<C>>>>,
    stream_handlers: Arc<Mutex<HashMap<u64, RpcStreamHandler<C>>>>,
    _context: PhantomData<C>,
}

impl<C> Default for RpcServiceEndpoint<C>
where
    C: Send + Sync + Clone + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<C> RpcServiceEndpoint<C>
where
    C: Send + Sync + Clone + 'static,
{
    /// Creates a new RPC service endpoint.
    pub fn new() -> Self {
        Self {
            prebuffered_handlers: Arc::new(Mutex::new(HashMap::new())),
            stream_handlers: Arc::new(Mutex::new(HashMap::new())),
            _context: PhantomData,
        }
    }
}

/// The implementation of the interface is now also written only once.
#[async_trait::async_trait]
impl<C> RpcServiceEndpointInterface<C> for RpcServiceEndpoint<C>
where
    C: Send + Sync + Clone + 'static,
{
    type HandlersLock = Mutex<HashMap<u64, RpcPrebufferedHandler<C>>>;
    type StreamHandlersLock = Mutex<HashMap<u64, RpcStreamHandler<C>>>;

    fn get_prebuffered_handlers(&self) -> Arc<Self::HandlersLock> {
        self.prebuffered_handlers.clone()
    }

    fn get_stream_handlers(&self) -> Arc<Self::StreamHandlersLock> {
        self.stream_handlers.clone()
    }
}
