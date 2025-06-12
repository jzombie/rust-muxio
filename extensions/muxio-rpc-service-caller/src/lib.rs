use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use muxio::frame::FrameEncodeError;
use muxio::rpc::{
    RpcDispatcher, RpcRequest, RpcResultStatus,
    rpc_internals::{RpcStreamEncoder, RpcStreamEvent, rpc_trait::RpcEmit},
};
use muxio_rpc_service::{
    RpcClientInterface,
    constants::{DEFAULT_RPC_STREAM_CHANNEL_BUFFER_SIZE, DEFAULT_SERVICE_MAX_CHUNK_SIZE},
};
use std::io;
use std::sync::Arc;

/// A trait that provides a generic, asynchronous interface for accessing a shared
/// `RpcDispatcher` that may be protected by different kinds of mutexes.
///
/// ## The Problem This Solves
///
/// This trait solves the challenge of writing a single generic function that can
/// operate on an `RpcDispatcher` protected by either a `tokio::sync::Mutex` (for
/// native async code) or a `std::sync::Mutex` (for single-threaded WASM).
///
/// These two mutex types have incompatible lock guards (`tokio`'s is `Send`,
/// `std`'s is not), which prevents a simpler generic approach.
///
/// ## The Closure-Passing Pattern
///
/// Instead of trying to return a generic lock guard, this trait uses a
/// closure-passing pattern. The caller provides the work to be done via a
/// closure (`f`), and the implementation of this trait is responsible for:
///
/// 1. Acquiring the lock using its specific strategy (blocking or async).
/// 2. Executing the closure with a mutable reference to the locked data.
/// 3. Releasing the lock.
///
/// This encapsulates the locking logic and completely avoids the `Send` guard issue.
#[async_trait::async_trait]
pub trait WithDispatcher: Send + Sync {
    /// Executes a closure against the locked `RpcDispatcher`.
    ///
    /// # Type Parameters
    ///
    /// - `F`: A closure that takes `&mut RpcDispatcher` and is only called once.
    ///   It must be `Send` as the work may be moved to another thread.
    /// - `R`: The return type of the closure. It must be `Send` so the result can
    ///   be safely returned across `.await` points.
    async fn with_dispatcher<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut RpcDispatcher<'static>) -> R + Send,
        R: Send;
}

/// The implementation for Tokio's asynchronous mutex.
#[async_trait::async_trait]
impl WithDispatcher for tokio::sync::Mutex<RpcDispatcher<'static>> {
    async fn with_dispatcher<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut RpcDispatcher<'static>) -> R + Send,
        R: Send,
    {
        // Asynchronously acquires the lock without blocking the thread.
        let mut guard = self.lock().await;

        // Executes the provided work.
        f(&mut guard)
    }
}

/// The implementation for the standard library's blocking mutex.
#[async_trait::async_trait]
impl WithDispatcher for std::sync::Mutex<RpcDispatcher<'static>> {
    async fn with_dispatcher<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut RpcDispatcher<'static>) -> R + Send,
        R: Send,
    {
        // This blocks the current thread, which is fine for the single-threaded WASM context.
        // In a Tokio context, this would ideally use `spawn_blocking`, but that would
        // bind this generic library to a specific runtime. This simple implementation
        // is correct for its intended use cases.
        // TODO: Don't use expect or unwrap
        let mut guard = self.lock().expect("Mutex was poisoned");

        // Executes the provided work.
        f(&mut guard)
    }
}

// Modify the generic dipatcher to use the new trait
pub async fn call_rpc_streaming_generic<L>(
    dispatcher: Arc<L>,
    on_emit: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
    method_id: u64,
    payload: &[u8],
    is_finalized: bool,
) -> Result<
    (
        RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
        mpsc::Receiver<Vec<u8>>,
    ),
    io::Error,
>
where
    // The dispatcher holder `L` must implement our new trait.
    L: WithDispatcher,
{
    let (tx, rx) = mpsc::channel::<Vec<u8>>(DEFAULT_RPC_STREAM_CHANNEL_BUFFER_SIZE);
    let tx = Arc::new(std::sync::Mutex::new(Some(tx)));

    let (ready_tx, ready_rx) = oneshot::channel::<Result<(), io::Error>>();
    let ready_tx = Arc::new(std::sync::Mutex::new(Some(ready_tx)));

    let send_fn: Box<dyn RpcEmit + Send + Sync> = Box::new({
        let on_emit = on_emit.clone();
        move |chunk: &[u8]| {
            on_emit(chunk.to_vec());
        }
    });

    let recv_fn: Box<dyn FnMut(RpcStreamEvent) + Send + 'static> = Box::new({
        let tx = Arc::clone(&tx);
        let ready_tx = Arc::clone(&ready_tx);

        move |evt| match evt {
            RpcStreamEvent::Header { rpc_header, .. } => {
                let result_status = rpc_header
                    .metadata_bytes
                    .first()
                    .copied()
                    .and_then(|b| RpcResultStatus::try_from(b).ok())
                    .unwrap_or(RpcResultStatus::Success);

                if result_status != RpcResultStatus::Success {
                    // TODO: Don't use unwrap
                    if let Some(tx) = ready_tx.lock().unwrap().take() {
                        let _ = tx.send(Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("RPC failed: {:?}", result_status),
                        )));
                    }
                    // TODO: Don't use unwrap
                    let _ = tx.lock().unwrap().take();
                } else {
                    // TODO: Don't use unwrap
                    let _ = ready_tx.lock().unwrap().take().map(|t| t.send(Ok(())));
                }
            }
            RpcStreamEvent::PayloadChunk { bytes, .. } => {
                // TODO: Don't use unwrap
                if let Some(sender) = tx.lock().unwrap().as_mut() {
                    let _ = sender.try_send(bytes);
                }
            }
            RpcStreamEvent::End { .. } => {
                // TODO: Don't use unwrap
                let _ = tx.lock().unwrap().take();
            }
            _ => {}
        }
    });

    // We pass a closure to the dispatcher.
    // The lock guard is never exposed across an .await point.
    let rpc_call_result = dispatcher
        .with_dispatcher(|d| {
            d.call(
                RpcRequest {
                    method_id,
                    param_bytes: Some(payload.to_vec()),
                    prebuffered_payload_bytes: None,
                    is_finalized,
                },
                DEFAULT_SERVICE_MAX_CHUNK_SIZE,
                send_fn,
                Some(recv_fn),
                true,
            )
        })
        .await;

    let encoder = rpc_call_result.map_err(|e: FrameEncodeError| {
        io::Error::new(io::ErrorKind::Other, format!("Encode error: {e:?}"))
    })?;

    match ready_rx.await {
        Ok(Ok(())) => Ok((encoder, rx)),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::Other,
            "RPC response channel closed prematurely",
        )),
    }
}

pub async fn call_rpc_buffered_generic<C, T, F>(
    client: &C,
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
    C: RpcClientInterface + ?Sized,
    T: Send + 'static,
    F: Fn(&[u8]) -> T + Send + Sync + 'static,
{
    // 1. Call the streaming method from the provided client.
    let (encoder, mut stream) = client
        .call_rpc_streaming(method_id, payload, is_finalized)
        .await?;

    // 2. Collect all chunks from the stream into a single buffer.
    let mut buf = Vec::new();
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(&chunk);
    }

    // 3. Decode the buffer and return the result.
    Ok((encoder, Ok(decode(&buf))))
}
