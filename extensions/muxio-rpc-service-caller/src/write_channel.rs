use tokio::sync::mpsc;

/// Spawn a background task that drains an unbounded mpsc channel,
/// calling `handler` for each received message.
///
/// The handler receives each message and returns a future.  When the
/// future returns `Err(())` the loop exits.
///
/// # Why unbounded?
///
/// A bounded channel would push backpressure into the emit closure, which
/// is called synchronously from `write_bytes` inside `FrameStreamEncoder`.
/// Making the emit closure async would stall the producer (e.g. a PTY
/// reader) whenever the I/O task falls behind, causing head-of-line
/// blocking — one slow stream stalls **all** streams on this connection.
///
/// The proper fix is per-stream flow control (like HTTP/2 `WINDOW_UPDATE`):
/// the encoder checks a per-stream byte budget and queues frames when the
/// window closes while other streams keep flowing.  Simply switching to a
/// bounded channel here will cause latency spikes and can hang the producer
/// under load — do not do it without adding stream-level flow control first.
pub fn spawn_write_loop<M, F, Fut>(
    handler: F,
) -> (mpsc::UnboundedSender<M>, tokio::task::JoinHandle<()>)
where
    M: Send + 'static,
    F: Fn(M) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), ()>> + Send,
{
    let (tx, mut rx) = mpsc::unbounded_channel::<M>();

    let handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if handler(msg).await.is_err() {
                break;
            }
        }
    });

    (tx, handle)
}
