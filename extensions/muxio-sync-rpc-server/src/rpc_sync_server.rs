use muxio_core::{frame::FrameDecodeError, rpc::RpcDispatcher};
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::{RpcServiceEndpoint, RpcServiceEndpointInterface};
use std::{
    fmt,
    io::{Read, Write},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use tokio::sync::Mutex;

type RpcTransportStateChangeHandler =
    Arc<Mutex<Option<Box<dyn Fn(RpcTransportState) + Send + Sync>>>>;

/// A sync RPC server: no async runtime inside.
///
/// Drives one [`RpcServiceEndpoint`] over owned byte halves on std
/// threads: the writer drains the emit queue with blocking writes, the
/// reader feeds inbound bytes through the endpoint until EOF.
/// Callers pass the current process stdin/stdout halves themselves for
/// guests that ARE the server (see [`RpcSyncServer::stdio`]). Teardown is
/// reader-driven and sole-owned: EOF fires `Disconnected` exactly once
/// and fails pending requests. `Drop` performs no teardown of its own:
/// it cannot block (it may run on a runtime thread) and cannot interrupt
/// a generic blocking read, so transports die via peer EOF like the
/// client side does via child-kill.
pub struct RpcSyncServer {
    dispatcher: Arc<tokio::sync::Mutex<RpcDispatcher<'static>>>,
    endpoint: Arc<RpcServiceEndpoint<()>>,
    emit_tx: mpsc::Sender<Vec<u8>>,
    state_change_handler: RpcTransportStateChangeHandler,
    is_connected: Arc<AtomicBool>,
    disconnected_rx: StdMutex<mpsc::Receiver<()>>,
}

impl fmt::Debug for RpcSyncServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RpcSyncServer")
            .field("is_connected", &self.is_connected.load(Ordering::Relaxed))
            .finish()
    }
}

impl RpcSyncServer {
    /// Attach to owned pipe halves. No threads start here by design:
    /// inbound bytes arriving before handler registration would route to
    /// "method not found", so the consumer registers handlers on
    /// [`endpoint`](Self::endpoint) first and then calls
    /// [`start`](UnstartedServer::start).
    pub fn unstarted(
        reader: Box<dyn Read + Send + 'static>,
        writer: Box<dyn Write + Send + 'static>,
    ) -> UnstartedServer {
        let dispatcher = Arc::new(Mutex::new(RpcDispatcher::new()));
        let endpoint = Arc::new(RpcServiceEndpoint::new());
        let state_change_handler: RpcTransportStateChangeHandler =
            Arc::new(Mutex::new(None));
        let is_connected = Arc::new(AtomicBool::new(true));
        let disconnect_error = Arc::new(StdMutex::new(None::<String>));
        let (emit_tx, emit_rx) = mpsc::channel::<Vec<u8>>();
        let (disconnected_tx, disconnected_rx) = mpsc::channel::<()>();
        UnstartedServer {
            reader: Some(reader),
            writer: Some(writer),
            dispatcher,
            endpoint,
            emit_tx,
            emit_rx: Some(emit_rx),
            state_change_handler,
            is_connected,
            disconnect_error,
            disconnected_tx: Some(disconnected_tx),
            disconnected_rx,
        }
    }

    /// Bind the current process stdio: the guest shape, where this
    /// process IS the server and the host parents its stdin/stdout.
    pub fn stdio() -> UnstartedServer {
        Self::unstarted(Box::new(std::io::stdin()), Box::new(std::io::stdout()))
    }
}

/// A constructed but unstarted server: endpoint and dispatcher ready for
/// handler registration, pipe halves held, no threads running, no bytes
/// routed. Call [`start`](UnstartedServer::start) once every handler is
/// registered.
pub struct UnstartedServer {
    reader: Option<Box<dyn Read + Send + 'static>>,
    writer: Option<Box<dyn Write + Send + 'static>>,
    dispatcher: Arc<tokio::sync::Mutex<RpcDispatcher<'static>>>,
    endpoint: Arc<RpcServiceEndpoint<()>>,
    emit_tx: mpsc::Sender<Vec<u8>>,
    emit_rx: Option<mpsc::Receiver<Vec<u8>>>,
    state_change_handler: RpcTransportStateChangeHandler,
    is_connected: Arc<AtomicBool>,
    disconnect_error: Arc<StdMutex<Option<String>>>,
    disconnected_tx: Option<mpsc::Sender<()>>,
    disconnected_rx: mpsc::Receiver<()>,
}

impl UnstartedServer {
    /// The endpoint to register handlers on before starting.
    pub fn endpoint(&self) -> Arc<RpcServiceEndpoint<()>> {
        Arc::clone(&self.endpoint)
    }

    /// Start pump threads and return the live server. Call only after
    /// registering every stream handler: bytes arriving earlier would
    /// route to "method not found".
    pub fn start(mut self) -> Arc<RpcSyncServer> {
        let mut writer = self.writer.take().expect("server halves taken");
        let emit_rx = self.emit_rx.take().expect("server halves taken");
        std::thread::Builder::new()
            .name("muxio-sync-srv-write".to_string())
            .spawn(move || {
                for chunk in emit_rx {
                    if writer.write_all(&chunk).is_err() {
                        break;
                    }
                    if writer.flush().is_err() {
                        break;
                    }
                }
            })
            .expect("sync server writer thread spawn failed");

        let server = Arc::new(RpcSyncServer {
            dispatcher: Arc::clone(&self.dispatcher),
            endpoint: Arc::clone(&self.endpoint),
            emit_tx: self.emit_tx.clone(),
            state_change_handler: Arc::clone(&self.state_change_handler),
            is_connected: Arc::clone(&self.is_connected),
            disconnected_rx: StdMutex::new(self.disconnected_rx),
        });
        let disconnected_tx = self.disconnected_tx.take().expect("server halves taken");
        RpcSyncServer::spawn_reader(ReaderConfig {
            reader: self.reader.take().expect("server halves taken"),
            dispatcher: self.dispatcher,
            endpoint: self.endpoint,
            emit_tx: self.emit_tx,
            state_change_handler: self.state_change_handler,
            is_connected: self.is_connected,
            disconnect_error: self.disconnect_error,
            disconnected_tx,
        });
        server
    }
}

/// Reader thread inputs, bundled so `spawn_reader` stays under the
/// argument-count lint as the pump gains disconnect plumbing.
struct ReaderConfig {
    reader: Box<dyn Read + Send + 'static>,
    dispatcher: Arc<tokio::sync::Mutex<RpcDispatcher<'static>>>,
    endpoint: Arc<RpcServiceEndpoint<()>>,
    emit_tx: mpsc::Sender<Vec<u8>>,
    state_change_handler: RpcTransportStateChangeHandler,
    is_connected: Arc<AtomicBool>,
    disconnect_error: Arc<StdMutex<Option<String>>>,
    disconnected_tx: mpsc::Sender<()>,
}

impl RpcSyncServer {
    /// Block until the reader thread observes EOF. Used by hosts that own
    /// the server lifetime explicitly; guests call this after `start()`
    /// to serve until the host disconnects.
    pub fn join_reader(&self) {
        if let Ok(rx) = self.disconnected_rx.lock() {
            let _ = rx.recv();
        }
    }

    pub fn endpoint(&self) -> Arc<RpcServiceEndpoint<()>> {
        self.endpoint.clone()
    }

    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn spawn_reader(config: ReaderConfig) {
        let ReaderConfig {
            mut reader,
            dispatcher,
            endpoint,
            emit_tx,
            state_change_handler,
            is_connected,
            disconnect_error,
            disconnected_tx,
        } = config;
        std::thread::Builder::new()
            .name("muxio-sync-srv-read".to_string())
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
                // Sole teardown owner: this is the only thread that fires
                // the callback, so exactly-once needs no token. Blocking
                // primitives are safe here (plain std thread, never an
                // async context).
                is_connected.store(false, Ordering::SeqCst);
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
                let _ = disconnected_tx.send(());
            })
            .expect("sync server reader thread spawn failed");
    }
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcSyncServer {
    fn get_dispatcher(&self) -> Arc<tokio::sync::Mutex<RpcDispatcher<'static>>> {
        self.dispatcher.clone()
    }

    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        Arc::new({
            let tx = self.emit_tx.clone();
            move |chunk: Vec<u8>| {
                let _ = tx.send(chunk);
            }
        })
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

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

/// Server-side caller handle for server-initiated calls over an established
/// sync connection. Wraps the shared server (its dispatcher routes
/// responses, its emit half carries requests) so fixture `connect_s2c`
/// shapes match the other transports.
#[derive(Clone)]
pub struct RpcSyncServerHandle(pub Arc<RpcSyncServer>);

#[async_trait::async_trait]
impl RpcServiceCallerInterface for RpcSyncServerHandle {
    fn get_dispatcher(&self) -> Arc<tokio::sync::Mutex<RpcDispatcher<'static>>> {
        self.0.get_dispatcher()
    }

    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        self.0.get_emit_fn()
    }

    fn is_connected(&self) -> bool {
        self.0.is_connected()
    }

    async fn set_state_change_handler(
        &self,
        handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        self.0.set_state_change_handler(handler).await;
    }
}
