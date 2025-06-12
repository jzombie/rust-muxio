use super::{RpcServiceEndpointInterface, with_handlers_trait::WithHandlers};
use muxio::rpc::RpcDispatcher;
use muxio_rpc_service_caller::WithDispatcher;
use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};
use tokio::sync::Mutex;

/// The concrete handler type, wrapped in an Arc for shared ownership.
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

/// A concrete RPC service endpoint that uses Tokio's asynchronous mutex.
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

/// The implementation simply provides the required getters. All complex logic
/// is inherited from the trait's default methods.
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
