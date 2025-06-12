use super::endpoint_interface::RpcServiceEndpointInterface;
use muxio::rpc::RpcDispatcher;
use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc}; // <-- Ensure Arc is in use
use tokio::sync::Mutex;

// FIX: The handler type alias now uses Arc instead of Box.
// This makes the handler's ownership shared and its pointer cloneable.
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

/// A concrete implementation of an RPC service endpoint.
/// It holds the state required by the `RpcServiceEndpointInterface`.
pub struct RpcServiceEndpoint {
    prebuffered_handlers: Arc<Mutex<HashMap<u64, RpcPrebufferedHandler>>>,
    rpc_dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
}

impl RpcServiceEndpoint {
    pub fn new() -> Self {
        Self {
            prebuffered_handlers: Arc::new(Mutex::new(HashMap::new())),
            rpc_dispatcher: Arc::new(Mutex::new(RpcDispatcher::new())),
        }
    }
}

// The implementation is now trivial. It just provides access to its state.
#[async_trait::async_trait]
impl RpcServiceEndpointInterface for RpcServiceEndpoint {
    fn get_dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>> {
        self.rpc_dispatcher.clone()
    }

    fn get_prebuffered_handlers(&self) -> Arc<Mutex<HashMap<u64, RpcPrebufferedHandler>>> {
        self.prebuffered_handlers.clone()
    }
}
