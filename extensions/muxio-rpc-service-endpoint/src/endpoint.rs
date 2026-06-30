use super::RpcServiceEndpointInterface;
use muxio_core::rpc::rpc_internals::{RpcStreamEvent, rpc_trait::RpcEmit};
use std::collections::HashMap;
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

/// A streaming RPC handler receives individual [`RpcStreamEvent`]s as they
/// arrive from the transport, along with an emit function for sending response
/// chunks back to the caller and the connection context.
///
/// Unlike a prebuffered handler (which accumulates the entire request before
/// invoking the handler), a streaming handler is called synchronously for each
/// event — `Header`, `PayloadChunk`, `End`, and `Error`. The emit function
/// should be used to stream response chunks back.
pub type RpcStreamHandler<C> = Arc<
    dyn Fn(RpcStreamEvent, Box<dyn RpcEmit + Send + Sync>, C) + Send + Sync,
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
