use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use muxio::rpc::{
    RpcDispatcher, RpcRequest, RpcResultStatus,
    rpc_internals::{RpcStreamEncoder, RpcStreamEvent, rpc_trait::RpcEmit},
};
use muxio_rpc_service::{RpcClientInterface, constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE};
use muxio_rpc_service_caller::call_rpc_streaming_generic;
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
impl RpcClientInterface for RpcWasmClient {
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
        let (encoder, mut stream) = self
            .call_rpc_streaming(method_id, payload, is_finalized)
            .await?;

        let mut buf = Vec::new();
        while let Some(chunk) = stream.next().await {
            buf.extend_from_slice(&chunk);
        }

        Ok((encoder, Ok(decode(&buf))))
    }
}
