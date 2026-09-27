use muxio_core::{frame::FrameDecodeError, rpc::RpcDispatcher};
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
use std::{
    fmt,
    io::{Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use tokio::sync::Mutex;
use tracing::{self, instrument};

type RpcTransportStateChangeHandler =
    Arc<Mutex<Option<Box<dyn Fn(RpcTransportState) + Send + Sync>>>>;

/// A sync RPC client: no async runtime inside.
///
/// Owns one half of a byte pipe (or a spawned child speaking the framing
/// protocol on its stdio) and pumps both directions on std threads: a
/// writer thread drains the emit queue with blocking writes, a reader
/// thread feeds inbound bytes through the endpoint. Dropping the client
/// fails every pending request, fires `Disconnected`, and terminates the
/// child when spawned via [`RpcSyncClient::spawn`].
pub struct RpcSyncClient {
    dispatcher: Arc<tokio::sync::Mutex<RpcDispatcher<'static>>>,
    endpoint: Arc<RpcServiceEndpoint<()>>,
    emit_tx: mpsc::Sender<Vec<u8>>,
    state_change_handler: RpcTransportStateChangeHandler,
    is_connected: Arc<AtomicBool>,
    child: Arc<StdMutex<Option<Child>>>,
}

impl fmt::Debug for RpcSyncClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RpcSyncClient")
            .field("is_connected", &self.is_connected.load(Ordering::Relaxed))
            .finish()
    }
}

impl Drop for RpcSyncClient {
    fn drop(&mut self) {
        tracing::debug!("RpcSyncClient is being dropped. Shutting down.");
        if let Ok(mut guard) = self.child.lock()
            && let Some(mut child) = guard.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.shutdown_sync();
    }
}

impl RpcSyncClient {
    /// Attach to owned pipe halves. Reader and writer threads start
    /// immediately; the first `read` returning `Ok(0)` (EOF) or any read
    /// error drives the disconnect path exactly once.
    pub fn new(
        reader: Box<dyn Read + Send + 'static>,
        writer: Box<dyn Write + Send + 'static>,
    ) -> Arc<Self> {
        let dispatcher = Arc::new(Mutex::new(RpcDispatcher::new()));
        let endpoint = Arc::new(RpcServiceEndpoint::new());
        let state_change_handler: RpcTransportStateChangeHandler = Arc::new(Mutex::new(None));
        let is_connected = Arc::new(AtomicBool::new(true));
        let disconnect_error = Arc::new(StdMutex::new(None::<String>));
        let (emit_tx, emit_rx) = mpsc::channel::<Vec<u8>>();

        std::thread::Builder::new()
            .name("muxio-sync-write".to_string())
            .spawn(move || {
                let mut writer = writer;
                for chunk in emit_rx {
                    if writer.write_all(&chunk).is_err() {
                        break;
                    }
                    if writer.flush().is_err() {
                        break;
                    }
                }
            })
            .expect("sync writer thread spawn failed");

        let client = Arc::new(Self {
            dispatcher: Arc::clone(&dispatcher),
            endpoint: Arc::clone(&endpoint),
            emit_tx: emit_tx.clone(),
            state_change_handler: Arc::clone(&state_change_handler),
            is_connected: Arc::clone(&is_connected),
            child: Arc::new(StdMutex::new(None)),
        });

        Self::spawn_reader(
            reader,
            dispatcher,
            endpoint,
            emit_tx,
            Arc::clone(&is_connected),
            Arc::clone(&disconnect_error),
            Arc::clone(&state_change_handler),
        );
        client
    }

    /// Attach to the current process stdio: the inverse guest shape, for
    /// processes whose parent speaks the protocol over their stdin/stdout.
    pub fn stdio() -> Arc<Self> {
        Self::new(Box::new(std::io::stdin()), Box::new(std::io::stdout()))
    }

    /// Spawn `program` with `args` plus piped stdio and attach to the
    /// child halves. The child is killed on client drop.
    pub fn spawn(program: &str, args: &[&str]) -> Result<Arc<Self>, std::io::Error> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin: ChildStdin = child.stdin.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "child has no piped stdin")
        })?;
        let stdout: ChildStdout = child.stdout.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "child has no piped stdout")
        })?;
        let client = Self::new(Box::new(stdout), Box::new(stdin));
        if let Ok(mut guard) = client.child.lock() {
            guard.replace(child);
        }
        Ok(client)
    }

    fn spawn_reader(
        mut reader: Box<dyn Read + Send + 'static>,
        dispatcher: Arc<tokio::sync::Mutex<RpcDispatcher<'static>>>,
        endpoint: Arc<RpcServiceEndpoint<()>>,
        emit_tx: mpsc::Sender<Vec<u8>>,
        is_connected: Arc<AtomicBool>,
        disconnect_error: Arc<StdMutex<Option<String>>>,
        state_change_handler: RpcTransportStateChangeHandler,
    ) {
        std::thread::Builder::new()
            .name("muxio-sync-read".to_string())
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            if let Ok(mut guard) = disconnect_error.lock()
                                && guard.is_none()
                            {
                                *guard = Some("unexpected EOF (transport closed)".to_string());
                            }
                            break;
                        }
                        Ok(n) => {
                            let emit_tx = emit_tx.clone();
                            let mut dispatcher_guard =
                                futures_executor::block_on(dispatcher.lock());
                            let _ = futures_executor::block_on(endpoint.read_bytes(
                                &mut dispatcher_guard,
                                (),
                                &buf[..n],
                                move |chunk: &[u8]| {
                                    let _ = emit_tx.send(chunk.to_vec());
                                },
                            ));
                        }
                        Err(_) => {
                            break;
                        }
                    }
                }
                if is_connected.swap(false, Ordering::SeqCst) {
                    let guard = state_change_handler.blocking_lock();
                    if let Some(handler) = guard.as_ref() {
                        handler(RpcTransportState::Disconnected);
                    }
                    let err = disconnect_error
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .clone()
                        .map(FrameDecodeError::Transport)
                        .unwrap_or(FrameDecodeError::ReadAfterCancel);
                    futures_executor::block_on(dispatcher.lock()).fail_all_pending_requests(err);
                }
            })
            .expect("sync reader thread spawn failed");
    }

    fn shutdown_sync(&self) {
        // try_lock, never blocking_lock: Drop may run on a runtime
        // thread, where blocking panics. A contended handler simply
        // misses this best-effort teardown signal.
        if self.is_connected.swap(false, Ordering::SeqCst)
            && let Ok(guard) = self.state_change_handler.try_lock()
            && let Some(handler) = guard.as_ref()
        {
            handler(RpcTransportState::Disconnected);
        }
    }

    pub fn get_endpoint(&self) -> Arc<RpcServiceEndpoint<()>> {
        self.endpoint.clone()
    }
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcSyncClient {
    fn get_dispatcher(&self) -> Arc<tokio::sync::Mutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    #[instrument(skip(self))]
    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        Arc::new({
            let tx = self.emit_tx.clone();
            let is_connected = Arc::clone(&self.is_connected);
            move |chunk: Vec<u8>| {
                if !is_connected.load(Ordering::Relaxed) {
                    tracing::warn!("RpcSyncClient is disconnected, dropping outgoing RPC data.");
                    return;
                }
                let _ = tx.send(chunk);
            }
        })
    }

    #[instrument(skip(self, handler))]
    async fn set_state_change_handler(
        &self,
        handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        let mut state_handler = self.state_change_handler.lock().await;
        *state_handler = Some(Box::new(handler));
        if self.is_connected.load(Ordering::Relaxed) {
            if let Some(h) = state_handler.as_ref() {
                h(RpcTransportState::Connected);
            }
        } else if let Some(h) = state_handler.as_ref() {
            // Late registration on a dead transport reports the terminal
            // state immediately; otherwise the observer would wait for an
            // EOF that already happened.
            h(RpcTransportState::Disconnected);
        }
    }
}
