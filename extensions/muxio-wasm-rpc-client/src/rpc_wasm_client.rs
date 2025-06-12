use muxio::rpc::RpcDispatcher;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, WithDispatcher};
use std::io;
use std::sync::{Arc, Mutex};

pub struct RpcWasmClient {
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    emit_callback: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
}

impl RpcWasmClient {
    pub fn new(emit_callback: impl Fn(Vec<u8>) + Send + Sync + 'static) -> RpcWasmClient {
        RpcWasmClient {
            dispatcher: Arc::new(Mutex::new(RpcDispatcher::new())),
            emit_callback: Arc::new(emit_callback),
        }
    }

    // This helper fits the trait's requirement perfectly.
    fn dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    // This helper also fits the trait's requirement perfectly.
    fn emit(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        self.emit_callback.clone()
    }
}

// The new implementation block is now trivial.
#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcWasmClient {
    type DispatcherLock = Mutex<RpcDispatcher<'static>>;

    /// Provides the trait with access to this client's dispatcher.
    fn get_dispatcher(&self) -> Arc<Self::DispatcherLock> {
        self.dispatcher()
    }

    /// Provides the trait with this client's specific callback for sending
    /// bytes to the JavaScript host.
    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        self.emit()
    }
}
