use example_muxio_rpc_service_definition::{RpcMethodPrebuffered, prebuffered::Echo};
use muxio_rpc_service_caller::RpcServiceCallerInterface;
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_rpc_service_endpoint::RpcServiceEndpointInterface;
use muxio_sync_rpc_client::RpcSyncClient;
use muxio_sync_rpc_server::RpcSyncServer;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, mpsc};

struct DuplexReader {
    rx: mpsc::Receiver<Vec<u8>>,
    buffer: Vec<u8>,
    position: usize,
}

struct DuplexWriter {
    tx: mpsc::Sender<Vec<u8>>,
}
type PipePair = (Box<dyn Read + Send>, Box<dyn Write + Send>);
type DuplexEnds = (PipePair, PipePair);

fn duplex_pair() -> DuplexEnds {
    let (a_to_b_tx, a_to_b_rx) = mpsc::channel::<Vec<u8>>();
    let (b_to_a_tx, b_to_a_rx) = mpsc::channel::<Vec<u8>>();
    (
        (
            Box::new(DuplexReader {
                rx: b_to_a_rx,
                buffer: Vec::new(),
                position: 0,
            }),
            Box::new(DuplexWriter { tx: a_to_b_tx }),
        ),
        (
            Box::new(DuplexReader {
                rx: a_to_b_rx,
                buffer: Vec::new(),
                position: 0,
            }),
            Box::new(DuplexWriter { tx: b_to_a_tx }),
        ),
    )
}

impl Read for DuplexReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.position < self.buffer.len() {
                let n = (self.buffer.len() - self.position).min(buf.len());
                buf[..n].copy_from_slice(&self.buffer[self.position..self.position + n]);
                self.position += n;
                if self.position == self.buffer.len() {
                    self.buffer.clear();
                    self.position = 0;
                }
                return Ok(n);
            }
            match self.rx.recv() {
                Ok(chunk) => {
                    self.buffer = chunk;
                    self.position = 0;
                }
                Err(_) => return Ok(0),
            }
        }
    }
}

impl Write for DuplexWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.tx
            .send(buf.to_vec())
            .map(|_| buf.len())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "duplex peer gone"))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn echo_pair() -> Arc<RpcSyncClient> {
    let ((client_read, client_write), (server_read, server_write)) = duplex_pair();
    let setup = RpcSyncServer::setup(server_read, server_write);
    setup
        .endpoint()
        .register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
            let request_params = Echo::decode_request(&request_bytes)?;
            let response_bytes = Echo::encode_response(request_params)?;
            Ok(response_bytes)
        })
        .await
        .expect("register Echo");
    let server = setup.start();
    std::mem::forget(server);
    RpcSyncClient::new(client_read, client_write)
}

#[tokio::test]
async fn prebuffered_echo_round_trip() {
    let client = echo_pair().await;
    let payload = b"hello sync".to_vec();
    let response = Echo::call(&*client, payload.clone()).await.unwrap();
    assert_eq!(response, payload);
}

#[tokio::test]
async fn binary_safety_nul_and_crlf() {
    let client = echo_pair().await;
    let payload: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
    let response = Echo::call(&*client, payload.clone()).await.unwrap();
    assert_eq!(response, payload);
    let crlf = b"line1\r\nline2\r\n\x00trailer".to_vec();
    let response = Echo::call(&*client, crlf.clone()).await.unwrap();
    assert_eq!(response, crlf);
}

#[tokio::test]
async fn large_payload_streams_past_pipe_buffers() {
    let client = echo_pair().await;
    let payload = vec![42u8; 4 * 1024 * 1024];
    let response = Echo::call(&*client, payload.clone()).await.unwrap();
    assert_eq!(response, payload);
}

#[tokio::test]
async fn spawn_failure_is_io_error() {
    let result = RpcSyncClient::spawn("muxio-sync-nonexistent-binary-xyz", &[]);
    assert!(result.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn drop_kill_chain_fires_disconnect() {
    let client = RpcSyncClient::spawn("sh", &["-c", "read line"]).unwrap();
    let fired = Arc::new(Mutex::new(false));
    let fired_clone = Arc::clone(&fired);
    client
        .set_state_change_handler(move |state| {
            if state == muxio_rpc_service_caller::RpcTransportState::Disconnected {
                *fired_clone.lock().unwrap() = true;
            }
        })
        .await;
    drop(client);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        *fired.lock().unwrap(),
        "dropping the client must kill sh and fire Disconnected"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn eof_from_peer_fires_disconnect() {    let ((client_read, client_write), (server_read, _server_write)) = duplex_pair();
    drop(server_read);
    let client = RpcSyncClient::new(client_read, client_write);
    let fired = Arc::new(Mutex::new(false));
    let fired_clone = Arc::clone(&fired);
    client
        .set_state_change_handler(move |state| {
            if state == muxio_rpc_service_caller::RpcTransportState::Disconnected {
                *fired_clone.lock().unwrap() = true;
            }
        })
        .await;
    drop(client);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(*fired.lock().unwrap());
}

#[tokio::test]
async fn server_disconnect_handler_fires_on_peer_eof() {
    // The server side of the state-callback contract: closing the peer
    // halves drives server-reader EOF, which fires Disconnected.
    // (Dropping a client alone cannot do this: its pump threads own
    // their halves. Real teardown kills the child; here we drop the
    // raw halves directly, which the test owns until then.)
    let ((client_read, client_write), (server_read, server_write)) = duplex_pair();
    let server = RpcSyncServer::setup(server_read, server_write).start();
    let fired = Arc::new(Mutex::new(false));
    let fired_clone = Arc::clone(&fired);
    server
        .set_state_change_handler(move |state| {
            if state == muxio_rpc_service_caller::RpcTransportState::Disconnected {
                *fired_clone.lock().unwrap() = true;
            }
        })
        .await;
    drop((client_read, client_write));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        *fired.lock().unwrap(),
        "server must fire Disconnected on peer EOF"
    );
}

#[tokio::test]
async fn server_late_handler_gets_terminal_state() {
    // Registering after transport death reports Disconnected
    // immediately instead of waiting for an EOF that already happened.
    let ((client_read, client_write), (server_read, server_write)) = duplex_pair();
    let server = RpcSyncServer::setup(server_read, server_write).start();
    drop((client_read, client_write));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let fired = Arc::new(Mutex::new(false));
    let fired_clone = Arc::clone(&fired);
    server
        .set_state_change_handler(move |state| {
            if state == muxio_rpc_service_caller::RpcTransportState::Disconnected {
                *fired_clone.lock().unwrap() = true;
            }
        })
        .await;
    assert!(
        *fired.lock().unwrap(),
        "late server handler must observe Disconnected at once"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn client_late_handler_gets_terminal_state() {
    // Same contract on the client: a child that already exited reports
    // Disconnected to a handler registered after the fact.
    let client = RpcSyncClient::spawn("sh", &["-c", "exit 0"]).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let fired = Arc::new(Mutex::new(false));
    let fired_clone = Arc::clone(&fired);
    client
        .set_state_change_handler(move |state| {
            if state == muxio_rpc_service_caller::RpcTransportState::Disconnected {
                *fired_clone.lock().unwrap() = true;
            }
        })
        .await;
    assert!(
        *fired.lock().unwrap(),
        "late client handler must observe Disconnected at once"
    );
}
