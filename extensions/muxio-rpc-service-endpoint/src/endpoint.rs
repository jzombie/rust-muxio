use super::RpcServiceEndpointInterface;
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
            C,
            Vec<u8>,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

/// A concrete RPC service endpoint, generic over a context type `C`.
pub struct RpcServiceEndpoint<C>
where
    C: Send + Sync + Clone + 'static,
{
    prebuffered_handlers: Arc<Mutex<HashMap<u64, RpcPrebufferedHandler<C>>>>,
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

    fn get_prebuffered_handlers(&self) -> Arc<Self::HandlersLock> {
        self.prebuffered_handlers.clone()
    }
}
