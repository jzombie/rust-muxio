use bytes::Bytes;
use interprocess::local_socket::{
    GenericNamespaced, ListenerOptions, ToNsName,
    tokio::{Listener, prelude::*},
};
use muxio_core::frame::FrameDecodeError;
use muxio_core::rpc::RpcDispatcher;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::io::AsyncReadExt;
use tokio::sync::{Mutex, mpsc};

static NEXT_CONN_ID: AtomicUsize = AtomicUsize::new(1);

/// Represents events that occur on the `RpcIpcServer`.
pub enum RpcIpcServerEvent {
    ClientConnected(RpcIpcConnectionContextHandle),
    ClientDisconnected(usize),
}

pub struct RpcIpcConnectionContext {
    pub write_tx: mpsc::UnboundedSender<Vec<u8>>,
    pub conn_id: usize,
    pub is_connected: Arc<AtomicBool>,
    pub dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
}

/// A wrapper around `Arc<RpcIpcConnectionContext>` to satisfy Rust's orphan rule.
#[derive(Clone)]
pub struct RpcIpcConnectionContextHandle(pub Arc<RpcIpcConnectionContext>);

/// An RPC server that listens for local socket connections and handles RPC calls.
pub struct RpcIpcServer {
    endpoint: Arc<RpcServiceEndpoint<Arc<RpcIpcConnectionContext>>>,
    event_tx: Option<mpsc::UnboundedSender<RpcIpcServerEvent>>,
}

impl RpcIpcServer {
    /// Creates a new `RpcIpcServer`.
    pub fn new(event_tx: Option<mpsc::UnboundedSender<RpcIpcServerEvent>>) -> Self {
        RpcIpcServer {
            endpoint: Arc::new(RpcServiceEndpoint::new()),
            event_tx,
        }
    }

    /// Returns an `Arc` clone of the underlying RPC service endpoint.
    pub fn endpoint(&self) -> Arc<RpcServiceEndpoint<Arc<RpcIpcConnectionContext>>> {
        self.endpoint.clone()
    }

    /// Starts the RPC server bound to the given local socket path.
    ///
    /// On Unix, this will create a Unix domain socket at the given path.
    /// On Windows, it will create a named pipe with the given name.
    ///
    /// If a stale socket file exists (e.g., from a previous crash), it will
    /// be automatically overwritten.
    pub async fn serve(self, socket_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let name = socket_path.to_ns_name::<GenericNamespaced>()?;
        let listener = ListenerOptions::new()
            .name(name)
            .try_overwrite(true)
            .create_tokio()?;
        tracing::info!("RPC-IPC server listening on {:?}", socket_path);
        let server = Arc::new(self);
        server.serve_with_listener(listener).await
    }

    /// Starts the RPC server with a pre-created `LocalSocketListener`.
    pub async fn serve_with_listener(
        self: Arc<Self>,
        listener: Listener,
    ) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            let conn = match listener.accept().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to accept RPC-IPC connection: {:?}", e);
                    continue;
                }
            };
            let server_clone = self.clone();
            tokio::spawn(async move {
                server_clone.handle_connection(conn).await;
            });
        }
    }

    async fn handle_connection(self: Arc<Self>, conn: LocalSocketStream) {
        let (mut read_half, write_half) = tokio::io::split(conn);
        let write_half = std::sync::Arc::new(tokio::sync::Mutex::new(write_half));
        let (tx_mpsc, writer_handle) =
            muxio_rpc_service_caller::write_channel::spawn_write_loop(move |msg: Vec<u8>| {
                let w = write_half.clone();
                async move {
                    use tokio::io::AsyncWriteExt;
                    w.lock().await.write_all(&msg).await.map_err(|_| ())
                }
            });
        let is_connected = Arc::new(AtomicBool::new(true));
        let conn_id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
        let peer_label = conn_id;

        let context = Arc::new(RpcIpcConnectionContext {
            write_tx: tx_mpsc.clone(),
            conn_id,
            is_connected: is_connected.clone(),
            dispatcher: Arc::new(Mutex::new(RpcDispatcher::new())),
        });

        if let Some(tx_event) = &self.event_tx {
            let _ = tx_event.send(RpcIpcServerEvent::ClientConnected(RpcIpcConnectionContextHandle(
                context.clone(),
            )));
        }

        let context_for_reader = context.clone();
        let self_for_reader = self.clone();
        let reader_handle = tokio::spawn(async move {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match read_half.read(&mut buf).await {
                    Ok(0) => {
                        tracing::info!("RPC-IPC client {} disconnected (EOF).", peer_label);
                        break;
                    }
                    Ok(n) => {
                        let bytes = Bytes::copy_from_slice(&buf[..n]);
                        let mut dispatcher = context_for_reader.dispatcher.lock().await;
                        let tx_clone = tx_mpsc.clone();
                        let on_emit = move |chunk: &[u8]| {
                            let _ = tx_clone.send(chunk.to_vec());
                        };
                        if let Err(err) = self_for_reader
                            .endpoint
                            .read_bytes(
                                &mut dispatcher,
                                context_for_reader.clone(),
                                &bytes,
                                on_emit,
                            )
                            .await
                        {
                            tracing::error!(
                                "RPC-IPC server: error processing bytes from {}: {:?}",
                                peer_label,
                                err
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!("RPC-IPC server: read error from {}: {:?}", peer_label, e);
                        break;
                    }
                }
            }
        });

        tokio::select! {
            _ = writer_handle => {},
            _ = reader_handle => {},
        }

        is_connected.store(false, Ordering::SeqCst);
        let mut dg = context.dispatcher.lock().await;
        dg.fail_all_pending_requests(FrameDecodeError::ReadAfterCancel);
        if let Some(tx_event) = &self.event_tx {
            let _ = tx_event.send(RpcIpcServerEvent::ClientDisconnected(conn_id));
        }
    }
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcIpcConnectionContextHandle {
    fn get_dispatcher(&self) -> Arc<Mutex<RpcDispatcher<'static>>> {
        self.0.dispatcher.clone()
    }

    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        Arc::new({
            let write_tx = self.0.write_tx.clone();
            move |chunk: Vec<u8>| {
                let _ = write_tx.send(chunk);
            }
        })
    }

    fn is_connected(&self) -> bool {
        self.0.is_connected.load(Ordering::SeqCst)
    }

    async fn set_state_change_handler(
        &self,
        _handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        tracing::warn!(
            "set_state_change_handler called on server-side RPC-IPC connection context; this is a no-op."
        );
    }
}
