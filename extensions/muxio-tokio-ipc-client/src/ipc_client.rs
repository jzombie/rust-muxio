use interprocess::local_socket::{GenericNamespaced, ToNsName, tokio::prelude::*};
use muxio_core::{frame::FrameDecodeError, rpc::RpcDispatcher};
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::RpcServiceEndpoint;
use std::{
    fmt, io,
    sync::{
        Arc, Mutex as StdMutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{
    io::AsyncReadExt,
    sync::{Mutex as TokioMutex, mpsc},
    task::JoinHandle,
};
use tracing::{self, instrument};

type RpcTransportStateChangeHandler =
    Arc<StdMutex<Option<Box<dyn Fn(RpcTransportState) + Send + Sync>>>>;

pub struct IpcClient {
    dispatcher: Arc<TokioMutex<RpcDispatcher<'static>>>,
    endpoint: Arc<RpcServiceEndpoint<()>>,
    tx: mpsc::UnboundedSender<Vec<u8>>,
    state_change_handler: RpcTransportStateChangeHandler,
    is_connected: Arc<AtomicBool>,
    task_handles: Vec<JoinHandle<()>>,
}

impl fmt::Debug for IpcClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IpcClient")
            .field("is_connected", &self.is_connected.load(Ordering::Relaxed))
            .finish()
    }
}

impl Drop for IpcClient {
    #[instrument(skip(self))]
    fn drop(&mut self) {
        tracing::debug!("IpcClient is being dropped. Aborting tasks.");
        for handle in &self.task_handles {
            handle.abort();
        }
        self.shutdown_sync();
    }
}

impl IpcClient {
    #[instrument(skip(self))]
    fn shutdown_sync(&self) {
        if self.is_connected.swap(false, Ordering::SeqCst)
            && let Ok(guard) = self.state_change_handler.lock()
            && let Some(handler) = guard.as_ref()
        {
            handler(RpcTransportState::Disconnected);
        }
    }

    #[instrument(skip(self))]
    async fn shutdown_async(&self) {
        if self.is_connected.swap(false, Ordering::SeqCst) {
            if let Ok(guard) = self.state_change_handler.lock()
                && let Some(handler) = guard.as_ref()
            {
                handler(RpcTransportState::Disconnected);
            }
            let mut dispatcher = self.dispatcher.lock().await;
            dispatcher.fail_all_pending_requests(FrameDecodeError::ReadAfterCancel);
        }
    }

    #[instrument]
    pub async fn new(socket_path: &str) -> Result<Arc<Self>, io::Error> {
        let name = socket_path
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let stream = LocalSocketStream::connect(name)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;
        tracing::debug!("Connected to IPC server at {:?}", socket_path);

        let (read_half, write_half) = tokio::io::split(stream);
        let write_half = std::sync::Arc::new(tokio::sync::Mutex::new(write_half));
        let (app_tx, send_handle) =
            muxio_rpc_service_caller::write_channel::spawn_write_loop(move |msg: Vec<u8>| {
                let w = write_half.clone();
                async move {
                    use tokio::io::AsyncWriteExt;
                    w.lock().await.write_all(&msg).await.map_err(|_| ())
                }
            });

        let client = Arc::new_cyclic(|weak_client: &Weak<IpcClient>| {
            let state_change_handler: RpcTransportStateChangeHandler =
                Arc::new(StdMutex::new(None));
            let is_connected = Arc::new(AtomicBool::new(true));
            let dispatcher = Arc::new(TokioMutex::new(RpcDispatcher::new()));
            let endpoint = Arc::new(RpcServiceEndpoint::new());
            let mut task_handles = Vec::new();

            task_handles.push(send_handle);

            let weak_for_read = weak_client.clone();
            let read_stream = futures_util::stream::unfold(
                (read_half, vec![0u8; 64 * 1024]),
                |(mut r, mut buf)| async {
                    let n = r.read(&mut buf).await.ok()?;
                    if n == 0 {
                        None
                    } else {
                        Some((bytes::Bytes::copy_from_slice(&buf[..n]), (r, buf)))
                    }
                },
            );
            let emit_tx = app_tx.clone();
            let recv_handle =
                muxio_rpc_service_endpoint::client_read_channel::spawn_client_read_loop(
                    weak_for_read,
                    Box::pin(read_stream),
                    move |bytes: Vec<u8>| {
                        let _ = emit_tx.send(bytes);
                    },
                );
            task_handles.push(recv_handle);

            Self {
                dispatcher,
                endpoint,
                tx: app_tx,
                state_change_handler,
                is_connected,
                task_handles,
            }
        });

        Ok(client)
    }

    pub fn get_endpoint(&self) -> Arc<RpcServiceEndpoint<()>> {
        self.endpoint.clone()
    }
}

#[async_trait::async_trait]
impl muxio_rpc_service_endpoint::client_read_channel::ClientReadTarget for IpcClient {
    fn dispatcher(&self) -> Arc<TokioMutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }
    fn endpoint(&self) -> Arc<muxio_rpc_service_endpoint::RpcServiceEndpoint<()>> {
        self.endpoint.clone()
    }
    async fn shutdown(&self) {
        self.shutdown_async().await;
    }
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for IpcClient {
    fn get_dispatcher(&self) -> Arc<TokioMutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    #[instrument(skip(self))]
    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        Arc::new({
            let tx = self.tx.clone();
            let is_connected_clone = self.is_connected.clone();
            move |chunk: Vec<u8>| {
                if !is_connected_clone.load(Ordering::Relaxed) {
                    tracing::warn!("IpcClient is disconnected, dropping outgoing RPC data.");
                    return;
                }
                let chunk_len = chunk.len();
                let send_result = tx.send(chunk);
                match send_result {
                    Ok(_) => {
                        tracing::debug!("Emitted binary chunk ({} bytes) via mpsc.", chunk_len)
                    }
                    Err(e) => tracing::debug!(
                        "Failed to send binary chunk ({} bytes) via mpsc: {}",
                        chunk_len,
                        e
                    ),
                }
            }
        })
    }

    #[instrument(skip(self, handler))]
    async fn set_state_change_handler(
        &self,
        handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        let mut state_handler = self.state_change_handler.lock().unwrap();
        *state_handler = Some(Box::new(handler));
        if self.is_connected.load(Ordering::Relaxed)
            && let Some(h) = state_handler.as_ref()
        {
            h(RpcTransportState::Connected);
        }
    }
}
