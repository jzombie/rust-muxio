use crate::endpoint_helpers;
use crate::test_transport::TestTransport;
use async_trait::async_trait;
use muxio_core::rpc::rpc_internals::RpcStreamEvent;
use muxio_rpc_service_endpoint::RpcServiceEndpoint;
use muxio_sync_rpc_client::RpcSyncClient;
use muxio_sync_rpc_server::RpcSyncServer;
use std::{
    io::{Read, Write},
    sync::{Arc, Mutex, mpsc},
};
use tokio::sync::oneshot;

/// In-memory duplex byte pipe: two `Read`/`Write` halves with no OS
/// handles, so fixture tests stay hermetic. Blocking `Read` waits on
/// `mpsc::recv`; `Write` forwards into the peer queue; dropping all
/// writers delivers EOF.
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
    // Cross the halves: A reads what B writes and vice versa.
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

/// Serve one fixture connection: register standard handlers on the
/// setup-phase server endpoint. The caller starts pumping afterwards, so
/// no byte routes before handlers exist.
async fn serve_fixture(endpoint: &Arc<RpcServiceEndpoint<()>>) {
    endpoint_helpers::register_standard_handlers(&**endpoint).await;
    endpoint_helpers::register_error_handler(&**endpoint).await;
}

#[async_trait]
impl TestTransport for RpcSyncClient {
    type Client = RpcSyncClient;
    type S2cHandle = muxio_sync_rpc_server::RpcSyncServerHandle;

    fn name() -> &'static str {
        "sync"
    }

    async fn connect() -> (Arc<Self::Client>, Arc<RpcServiceEndpoint<()>>) {
        let ((client_read, client_write), (server_read, server_write)) = duplex_pair();
        let setup = RpcSyncServer::setup(server_read, server_write);
        serve_fixture(&setup.endpoint()).await;
        let server = setup.start();
        let client = RpcSyncClient::new(client_read, client_write);
        // The server has no socket to outlive the test: its pump threads
        // die on EOF when the client drops. Forgetting here mirrors the
        // spawned tokio servers, whose tasks run to test end.
        std::mem::forget(server);
        let endpoint = client.get_endpoint();
        (client, endpoint)
    }

    async fn connect_fail() -> Result<(), std::io::Error> {
        RpcSyncClient::spawn("muxio-sync-nonexistent-binary-xyz", &[]).map(|_| ())
    }

    async fn connect_with_disconnect() -> (Arc<Self::Client>, oneshot::Sender<()>) {
        let ((client_read, client_write), (_server_read, server_write)) = duplex_pair();
        let (tx, rx) = oneshot::channel();
        // Hold the server halves: dropping `server_write` on signal drives
        // client-side EOF, which the disconnect path reports.
        tokio::spawn(async move {
            let _ = rx.await;
            drop(server_write);
        });
        let client = RpcSyncClient::new(client_read, client_write);
        (client, tx)
    }

    async fn connect_s2c() -> (
        Arc<Self::Client>,
        Arc<RpcServiceEndpoint<()>>,
        Self::S2cHandle,
    ) {
        let ((client_read, client_write), (server_read, server_write)) = duplex_pair();
        let setup = RpcSyncServer::setup(server_read, server_write);
        serve_fixture(&setup.endpoint()).await;
        let server = setup.start();
        let client = RpcSyncClient::new(client_read, client_write);
        let endpoint = client.get_endpoint();
        (
            client,
            endpoint,
            muxio_sync_rpc_server::RpcSyncServerHandle(server),
        )
    }

    async fn connect_for_streaming() -> (
        Arc<Self::Client>,
        Arc<RpcServiceEndpoint<()>>,
        Arc<Mutex<Vec<RpcStreamEvent>>>,
    ) {
        let ((client_read, client_write), (server_read, server_write)) = duplex_pair();
        let setup = RpcSyncServer::setup(server_read, server_write);
        let endpoint = setup.endpoint();
        endpoint_helpers::register_standard_handlers(&*endpoint).await;
        endpoint_helpers::register_error_handler(&*endpoint).await;
        let captured = endpoint_helpers::register_stream_capture_handler(&*endpoint).await;
        let server = setup.start();
        std::mem::forget(server);
        let client = RpcSyncClient::new(client_read, client_write);
        let client_endpoint = client.get_endpoint();
        (client, client_endpoint, captured)
    }
}
