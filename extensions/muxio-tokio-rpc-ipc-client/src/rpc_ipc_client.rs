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

/// Warn threshold for the unbounded client write queue (see Phase 3.5:
/// backpressure is intentionally deferred, but depth is now visible).
const WRITE_QUEUE_WARN_THRESHOLD: usize = 1024;
static CLIENT_PENDING_WRITES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

type RpcTransportStateChangeHandler =
    Arc<StdMutex<Option<Box<dyn Fn(RpcTransportState) + Send + Sync>>>>;

pub struct RpcIpcClient {
    dispatcher: Arc<TokioMutex<RpcDispatcher<'static>>>,
    endpoint: Arc<RpcServiceEndpoint<()>>,
    tx: mpsc::UnboundedSender<Vec<u8>>,
    state_change_handler: RpcTransportStateChangeHandler,
    is_connected: Arc<AtomicBool>,
    task_handles: Vec<JoinHandle<()>>,
    disconnect_error: Arc<StdMutex<Option<String>>>,
}

impl fmt::Debug for RpcIpcClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RpcIpcClient")
            .field("is_connected", &self.is_connected.load(Ordering::Relaxed))
            .finish()
    }
}

impl Drop for RpcIpcClient {
    #[instrument(skip(self))]
    fn drop(&mut self) {
        tracing::debug!("RpcIpcClient is being dropped. Aborting tasks.");
        for handle in &self.task_handles {
            handle.abort();
        }
        self.shutdown_sync();
    }
}

impl RpcIpcClient {
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
        // Always mark disconnected and fail every pending stream, even if a
        // prior path already flipped the flag — otherwise a late disconnect
        // silently leaves client channels (and the UI waiting on them) hung.
        self.is_connected.store(false, Ordering::SeqCst);
        if let Ok(guard) = self.state_change_handler.lock()
            && let Some(handler) = guard.as_ref()
        {
            handler(RpcTransportState::Disconnected);
        }
        let err = {
            let guard = self
                .disconnect_error
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            match guard.clone() {
                Some(msg) => FrameDecodeError::Transport(msg),
                None => FrameDecodeError::ReadAfterCancel,
            }
        };
        let mut dispatcher = self.dispatcher.lock().await;
        dispatcher.fail_all_pending_requests(err);
    }

    #[instrument]
    pub async fn new(socket_path: &str) -> Result<Arc<Self>, io::Error> {
        let name = socket_path
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let stream = LocalSocketStream::connect(name)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;
        tracing::debug!("Connected to RPC-IPC server at {:?}", socket_path);

        let (read_half, write_half) = tokio::io::split(stream);
        let write_half = std::sync::Arc::new(tokio::sync::Mutex::new(write_half));
        let (app_tx, send_handle) =
            muxio_rpc_service_caller::write_channel::spawn_write_loop(move |msg: Vec<u8>| {
                let w = write_half.clone();
                async move {
                    use tokio::io::AsyncWriteExt;
                    let res = w.lock().await.write_all(&msg).await.map_err(|_| ());
                    // Decrement pending counter after the frame is flushed or dropped,
                    // guarding against underflow if the counter was already zero
                    // (e.g. after a failed send that already undid its increment).
                    let _ = CLIENT_PENDING_WRITES.fetch_update(
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                        |v| if v == 0 { None } else { Some(v - 1) },
                    );
                    res
                }
            });

        let client = Arc::new_cyclic(|weak_client: &Weak<RpcIpcClient>| {
            let state_change_handler: RpcTransportStateChangeHandler =
                Arc::new(StdMutex::new(None));
            let is_connected = Arc::new(AtomicBool::new(true));
            let dispatcher = Arc::new(TokioMutex::new(RpcDispatcher::new()));
            let endpoint = Arc::new(RpcServiceEndpoint::new());
            let disconnect_error = Arc::new(StdMutex::new(None::<String>));
            let mut task_handles = Vec::new();

            task_handles.push(send_handle);

            let weak_for_read = weak_client.clone();
            let disconnect_error_for_read = disconnect_error.clone();
            let read_stream = futures_util::stream::unfold(
                (read_half, vec![0u8; 64 * 1024]),
                move |(mut r, mut buf)| {
                    let disconnect_error = disconnect_error_for_read.clone();
                    async move {
                        let n = match r.read(&mut buf).await {
                            Ok(0) => {
                                if let Ok(mut guard) = disconnect_error.lock()
                                    && guard.is_none()
                                {
                                    *guard =
                                        Some("unexpected EOF (connection closed)".to_string());
                                }
                                tracing::debug!("RPC-IPC client read EOF");
                                return None;
                            }
                            Ok(n) => n,
                            Err(e) => {
                                tracing::warn!(error = ?e, "RPC-IPC client read failed");
                                if let Ok(mut guard) = disconnect_error.lock() {
                                    *guard = Some(e.to_string());
                                }
                                return None;
                            }
                        };
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
                disconnect_error,
            }
        });

        Ok(client)
    }

    pub fn get_endpoint(&self) -> Arc<RpcServiceEndpoint<()>> {
        self.endpoint.clone()
    }
}

#[async_trait::async_trait]
impl muxio_rpc_service_endpoint::client_read_channel::ClientReadTarget for RpcIpcClient {
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
impl RpcServiceCallerInterface for RpcIpcClient {
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
                    tracing::warn!("RpcIpcClient is disconnected, dropping outgoing RPC data.");
                    return;
                }
                let chunk_len = chunk.len();
                let pending =
                    CLIENT_PENDING_WRITES.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
                if pending > WRITE_QUEUE_WARN_THRESHOLD {
                    tracing::warn!(
                        pending_writes = pending,
                        threshold = WRITE_QUEUE_WARN_THRESHOLD,
                        "Client write queue depth exceeded threshold"
                    );
                }
                let send_result = tx.send(chunk);
                match send_result {
                    Ok(_) => {
                        tracing::debug!("Emitted binary chunk ({} bytes) via mpsc.", chunk_len)
                    }
                    Err(e) => {
                        // Decrement on failure to keep counter roughly accurate,
                        // guarding against underflow.
                        let _ = CLIENT_PENDING_WRITES.fetch_update(
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                            |v| if v == 0 { None } else { Some(v - 1) },
                        );
                        tracing::debug!(
                            "Failed to send binary chunk ({} bytes) via mpsc: {}",
                            chunk_len,
                            e
                        )
                    }
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
