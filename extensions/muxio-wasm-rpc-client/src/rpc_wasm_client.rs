use futures::channel::mpsc;
use muxio::rpc::{
    RpcDispatcher,
    rpc_internals::{RpcStreamEncoder, rpc_trait::RpcEmit},
};
use muxio_rpc_service_caller::RpcServiceCallerInterface;
use muxio_rpc_service_caller::{call_rpc_buffered_generic, call_rpc_streaming_generic};
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

    fn dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    fn emit(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        self.emit_callback.clone()
    }
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcWasmClient {
    type DispatcherMutex<T> = Mutex<RpcDispatcher<'static>>;

    fn get_dispatcher(&self) -> Arc<Self::DispatcherMutex<RpcDispatcher<'static>>> {
        self.dispatcher()
    }

    async fn call_rpc_streaming(
        &self,
        method_id: u64,
        payload: &[u8],
        is_finalized: bool,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            mpsc::Receiver<Vec<u8>>,
        ),
        io::Error,
    > {
        call_rpc_streaming_generic(
            self.dispatcher(),
            self.emit(),
            method_id,
            payload,
            is_finalized,
        )
        .await
    }

    async fn call_rpc_buffered<T, F>(
        &self,
        method_id: u64,
        payload: &[u8],
        decode: F,
        is_finalized: bool,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            Result<T, io::Error>,
        ),
        io::Error,
    >
    where
        T: Send + 'static,
        F: Fn(&[u8]) -> T + Send + Sync + 'static,
    {
        // Delegate directly to the generic buffered helper
        call_rpc_buffered_generic(self, method_id, payload, decode, is_finalized).await
    }
}
