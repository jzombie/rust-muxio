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
/// reader) whenever the I/O task falls behind — and because there's one
/// channel per **connection**, that stall blocks **all** streams on that
/// connection, not just the slow one.
///
/// # How to fix properly
///
/// Per-stream byte budgets in `FrameStreamEncoder`.  When a stream exceeds
/// its budget, `write_bytes` queues the frame locally instead of emitting
/// it — other streams' frames still pass through the unbounded channel
/// unaffected.  The framing layer already has `stream_id` on every frame;
/// the missing piece is:
///
/// 1. A per-stream credit counter in `RpcSession` or `FrameStreamEncoder`.
/// 2. A `WINDOW_UPDATE`-style message type so the receiver can grant more
///    credits when it has consumed data.
/// 3. A queue in the encoder for frames that couldn't be sent yet.
///
/// Simply switching this channel to bounded will cause latency spikes and
/// can hang the producer under load — do not do it without adding
/// per-stream budgets first.
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
