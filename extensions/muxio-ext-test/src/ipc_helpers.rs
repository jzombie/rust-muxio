use crate::endpoint_helpers::{
    register_error_handler, register_standard_handlers, register_stream_capture_handler,
};
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, ToNsName, tokio::Listener};
use muxio_core::rpc::rpc_internals::RpcStreamEvent;
use muxio_tokio_rpc_ipc_client::RpcIpcClient;
use muxio_tokio_rpc_ipc_server::{RpcIpcServer, RpcIpcServerEvent};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static IPC_HELPER_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn temp_socket_name(name: &str) -> String {
    let n = IPC_HELPER_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("muxio-ipc-test-{name}-{n}")
}

/// Binds the local-socket listener synchronously. The returned listener is
/// already bound, so a subsequently spawned `serve_with_listener` task is
/// guaranteed to accept connections — no sleep-based readiness wait needed.
fn bind_listener(socket_name: &str) -> Listener {
    let ns_name = socket_name
        .to_ns_name::<GenericNamespaced>()
        .expect("valid IPC socket name");
    ListenerOptions::new()
        .name(ns_name)
        .try_overwrite(true)
        .create_tokio()
        .expect("bind IPC listener")
}

pub async fn setup_ipc_server(test_name: &str) -> String {
    let name = temp_socket_name(test_name);
    // Bind before spawning: the listener exists by the time this returns,
    // so clients can connect deterministically without sleeping.
    let listener = bind_listener(&name);
    let server = RpcIpcServer::new(None);
    register_standard_handlers(&*server.endpoint()).await;
    register_error_handler(&*server.endpoint()).await;
    let server = Arc::new(server);
    tokio::spawn(async move {
        let _ = server.serve_with_listener(listener).await;
    });
    name
}

pub async fn setup_ipc_server_with_events(
    test_name: &str,
) -> (
    tokio::sync::mpsc::UnboundedReceiver<RpcIpcServerEvent>,
    String,
) {
    let name = temp_socket_name(test_name);
    // Bind before spawning: the listener exists by the time this returns.
    let listener = bind_listener(&name);
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let server = RpcIpcServer::new(Some(event_tx));
    register_standard_handlers(&*server.endpoint()).await;
    let server = Arc::new(server);
    tokio::spawn(async move {
        let _ = server.serve_with_listener(listener).await;
    });
    (event_rx, name)
}

/// Sets up an IPC server with the shared streaming-capture handler registered.
/// Returns the socket name and the captured-events buffer.
pub async fn setup_ipc_streaming_server(
    test_name: &str,
) -> (String, Arc<Mutex<Vec<RpcStreamEvent>>>) {
    let name = temp_socket_name(test_name);
    // Bind before spawning: the listener exists by the time this returns.
    let listener = bind_listener(&name);
    let server = RpcIpcServer::new(None);
    register_standard_handlers(&*server.endpoint()).await;
    register_error_handler(&*server.endpoint()).await;
    let events = register_stream_capture_handler(&*server.endpoint()).await;
    let server = Arc::new(server);
    tokio::spawn(async move {
        let _ = server.serve_with_listener(listener).await;
    });
    (name, events)
}

pub async fn connect_ipc_client(socket_path: &str) -> Arc<RpcIpcClient> {
    // Deterministic connect with bounded exponential backoff instead of a
    // blind pre-connect sleep. Only connection-refused style errors retry;
    // anything else fails fast.
    let mut delay = Duration::from_millis(10);
    let mut last_err = None;
    for _ in 0..50 {
        match RpcIpcClient::new(socket_path).await {
            Ok(client) => return client,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                ) =>
            {
                last_err = Some(e);
                tokio::time::sleep(delay).await;
                delay = std::cmp::min(delay * 2, Duration::from_millis(200));
            }
            Err(e) => panic!("IPC connect to {socket_path:?} failed: {e:?}"),
        }
    }
    panic!(
        "IPC connect to {socket_path:?} failed after retries: {:?}",
        last_err
    );
}
