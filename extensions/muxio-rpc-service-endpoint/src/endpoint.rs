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
/// from within a streaming handler.  Writes go directly to the transport
/// via a pre-created `RpcStreamEncoder` (set after `read_bytes` returns),
/// so the responder can be stored and used from background tasks.
#[derive(Clone)]
pub struct StreamResponder {
    pub(crate) request_id: u32,
    writer: Arc<StdMutex<Option<Box<dyn FnMut(&[u8], bool) + Send>>>>,
    buffer: Arc<StdMutex<Vec<(Vec<u8>, bool)>>>,
}

impl StreamResponder {
    pub(crate) fn new(request_id: u32) -> Self {
        Self {
            request_id,
            writer: Arc::new(StdMutex::new(None)),
            buffer: Arc::new(StdMutex::new(Vec::new())),
        }
    }

    /// Queue a response chunk.  Call with `is_finalized: true` on the
    /// last chunk to signal the end of the response stream.
    /// Writes directly to the transport if a writer is available,
    /// otherwise buffers until the writer is set (after `read_bytes`).
    pub fn respond(&self, chunk: Vec<u8>, is_finalized: bool) {
        let mut writer_guard = self
            .writer
            .lock()
            .expect("StreamResponder writer lock poisoned");
        if let Some(writer) = writer_guard.as_mut() {
            writer(&chunk, is_finalized);
        } else {
            self.buffer
                .lock()
                .expect("StreamResponder buffer lock poisoned")
                .push((chunk, is_finalized));
        }
    }

    pub(crate) fn set_writer(&self, writer: Box<dyn FnMut(&[u8], bool) + Send>) {
        let mut guard = self
            .writer
            .lock()
            .expect("StreamResponder writer lock poisoned");
        *guard = Some(writer);
        let buffered = std::mem::take(
            &mut *self
                .buffer
                .lock()
                .expect("StreamResponder buffer lock poisoned"),
        );
        if let Some(w) = guard.as_mut() {
            for (chunk, is_finalized) in buffered {
                w(&chunk, is_finalized);
            }
        }
    }
}

/// A streaming RPC handler receives individual [`RpcStreamEvent`]s as they
/// arrive from the transport, along with a [`StreamResponder`] for sending
/// properly framed response chunks back to the caller.
pub type RpcStreamHandler<C> = Arc<dyn Fn(RpcStreamEvent, StreamResponder, C) + Send + Sync>;

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
