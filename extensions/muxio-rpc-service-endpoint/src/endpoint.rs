use super::RpcServiceEndpointInterface;
use muxio::rpc::RpcDispatcher;
use std::collections::HashMap;
use std::{future::Future, pin::Pin, sync::Arc};

// --- Conditionally Alias the Mutex Implementation ---
// If the `tokio_support` feature is enabled, we alias `tokio::sync::Mutex`
// to be available locally as `Mutex`.
#[cfg(feature = "tokio_support")]
use tokio::sync::Mutex;

// If the `tokio_support` feature is NOT enabled, we use the standard
// library's blocking mutex instead.
#[cfg(not(feature = "tokio_support"))]
use std::sync::Mutex;

// --- Generic Definitions ---

/// A shareable, dynamically-dispatched asynchronous RPC handler.
/// This type is generic and used by all endpoint implementations.
pub type RpcPrebufferedHandler = Arc<
    dyn Fn(
            Vec<u8>,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

// --- Unified Struct Definition ---

/// A concrete RPC service endpoint.
///
/// This struct's underlying locking mechanism is determined at compile time
/// by the `tokio_support` feature flag. To the consumer of this crate,
/// it appears as a single, unified type.
pub struct RpcServiceEndpoint {
    // This `Mutex` will be either `tokio::sync::Mutex` or `std::sync::Mutex`
    // depending on the conditional `use` statement above.
    prebuffered_handlers: Arc<Mutex<HashMap<u64, RpcPrebufferedHandler>>>,
    rpc_dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
}

impl RpcServiceEndpoint {
    // TODO: Enable the ability to instantiate from a pre-existing dispatcher

    /// Creates a new RPC service endpoint.
    ///
    /// The underlying mutex will be chosen automatically based on whether the
    /// `tokio_support` feature is enabled for this crate.
    pub fn new() -> Self {
        Self {
            prebuffered_handlers: Arc::new(Mutex::new(HashMap::new())),
            rpc_dispatcher: Arc::new(Mutex::new(RpcDispatcher::new())),
        }
    }
}

/// The implementation of the interface is now also written only once.
/// The compiler ensures that the `DispatcherLock` and `HandlersLock`
/// associated types correctly resolve to the conditionally-aliased `Mutex`.
#[async_trait::async_trait]
impl RpcServiceEndpointInterface for RpcServiceEndpoint {
    type DispatcherLock = Mutex<RpcDispatcher<'static>>;
    type HandlersLock = Mutex<HashMap<u64, RpcPrebufferedHandler>>;

    fn get_dispatcher(&self) -> Arc<Self::DispatcherLock> {
        self.rpc_dispatcher.clone()
    }

    fn get_prebuffered_handlers(&self) -> Arc<Self::HandlersLock> {
        self.prebuffered_handlers.clone()
    }
}
