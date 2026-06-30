# FUTURE: Per-Stream Byte Budgets (Flow Control)

## Problem

The write channel between `FrameStreamEncoder` and the transport socket is
unbounded.  `write_bytes` → `emit_frame` → `on_emit(&[u8])` fires synchronously
into an `mpsc::unbounded_channel` (see `write_channel.rs:9-33`).  Under
sustained producer > consumer load the channel grows without bound, consuming
arbitrary memory.  This affects **all** transports (WS, IPC, TCP, UDP) that
use the same `spawn_write_loop` pattern.

Currently documented in `extensions/muxio-rpc-service-caller/src/write_channel.rs`
and `README.md` (lines 68-70).

## Design Goals

1. **Per-stream isolation** — stream A exhausting its budget only pauses
   stream A; other streams' frames continue flowing.
2. **No user-visible API change** — `write_bytes`, `flush`, `end_stream`
   signatures stay the same (though `write_bytes` may need to return more
   nuanced errors, e.g. `WouldBlock`).
3. **Transport agnostic** — works over TCP (stream-oriented), UDP
   (datagram-oriented), WebSocket (message-oriented), IPC (Unix pipes),
   and WASM bridge.
4. **Fairness** — no single stream hogs the shared budget; each stream gets
   its own allowance.
5. **Configurable** — budget size is settable at connection or method level.
6. **Backward compatible** — `budget: None` = current unbounded behaviour.

---

## Architecture

### New: `StreamBudgetController`

Location: `core/src/frame/stream_budget.rs`

```rust
struct StreamBudget {
    remaining: usize,
    pending: VecDeque<Vec<u8>>,  // frames buffered when over-budget
}

pub struct StreamBudgetController {
    per_stream: HashMap<u32, StreamBudget>,
    per_stream_limit: usize,          // default budget per stream
    on_emit: Box<dyn FnMut(&[u8])>,  // the real transport emitter
}
```

Methods:

```rust
impl StreamBudgetController {
    /// Try to emit `data` for `stream_id`.  Returns true if emitted,
    /// false if the stream is over-budget (data is buffered).
    fn try_emit(&mut self, stream_id: u32, data: &[u8]) -> bool;

    /// Called by the async write loop after each successful socket write.
    /// `bytes_written` is the number of bytes actually flushed to the
    /// transport, which replenishes the available budget.
    fn replenish(&mut self, bytes_written: usize);

    /// Flush any pending frames for streams whose budget has been
    /// replenished enough.
    fn flush_pending(&mut self);
}
```

**Budget accounting**:
- Each stream starts with `per_stream_limit` budget.
- `remaining` decrements on every `emit_frame` call (bytes emitted
  into the channel).
- `replenish(bytes_written)` distributes budget back proportionally
  across streams that have pending data — simplest approach: add
  `bytes_written` back and let the next `flush_pending` round drain
  whatever fits.
- A stream whose `remaining` hits zero buffers new frames in
  `pending` instead of calling `on_emit`.

### Changes to `FrameStreamEncoder`

File: `core/src/frame/frame_stream_encoder.rs`

```rust
pub struct FrameStreamEncoder<F> {
    stream_id: u32,
    max_chunk_size: usize,
    next_seq_id: u32,
    next_kind: FrameKind,
    buffer: Vec<u8>,           // unchanged — chunk assembly buffer
    is_canceled: bool,
    is_ended: bool,
    on_emit: F,
    budget: Option<Arc<Mutex<StreamBudgetController>>>,  // NEW
}
```

`write_bytes` changes from always-emitting to:

```rust
pub fn write_bytes(&mut self, data: &[u8]) -> Result<usize, FrameEncodeError> {
    // ... existing guards ...
    self.buffer.extend_from_slice(data);

    while self.buffer.len() >= self.max_chunk_size {
        let chunk = self.buffer.drain(..self.max_chunk_size).collect::<Vec<_>>();

        // Budget check — does this stream have allowance?
        if let Some(ref budget) = self.budget {
            let mut ctrl = budget.lock().unwrap();
            if !ctrl.try_emit(self.stream_id, &chunk) {
                // Over budget — put the chunk back and stop.
                // The remaining buffer stays; subsequent calls with
                // more data will extend it.
                for b in chunk.into_iter().rev() {
                    self.buffer.push_front(b);  // or prepend properly
                }
                break;
            }
            drop(ctrl); // release lock before emit_frame
        }

        let frame = Frame { /* ... */ };
        op_written_bytes += self.emit_frame(frame)?; // calls on_emit
    }
    Ok(op_written_bytes)
}
```

`flush` also needs to consult the budget:

```rust
pub fn flush(&mut self) -> Result<usize, FrameEncodeError> {
    if self.buffer.is_empty() { return Ok(0); }
    if let Some(ref budget) = self.budget {
        let ctrl = budget.lock().unwrap();
        if !ctrl.try_emit(self.stream_id, &self.buffer) {
            return Ok(0); // no budget — data stays buffered
        }
    }
    // ... emit frame as before ...
}
```

### Changes to `RpcStreamEncoder`

File: `core/src/rpc/rpc_internals/rpc_stream_encoder.rs`

Thread `budget: Option<Arc<Mutex<StreamBudgetController>>>` through:

```rust
pub fn new(
    stream_id: u32,
    max_chunk_size: usize,
    header: &RpcHeader,
    on_emit: F,
    budget: Option<Arc<Mutex<StreamBudgetController>>>,  // NEW
) -> Result<Self, FrameEncodeError> {
    let mut encoder = FrameStreamEncoder::new(stream_id, max_chunk_size, on_emit, budget);
    // ... header emission unchanged ...
}
```

### Changes to `RpcDispatcher`

File: `core/src/rpc/rpc_dispatcher.rs`

The `call()` method accepts the `budget` ref and passes it through:

```rust
pub fn call(
    &mut self,
    request: RpcRequest,
    max_chunk_size: usize,
    on_emit: Box<dyn RpcEmit + Send + Sync>,
    rpc_response_handler: Option<Box<dyn RpcResponseHandler + Send + 'static>>,
    prioritize: bool,
    budget: Option<Arc<Mutex<StreamBudgetController>>>,  // NEW
) -> Result<RpcStreamEncoder<impl FnMut(&[u8])>, RpcDispatchError>
```

### Changes to async write loop

File: `extensions/muxio-rpc-service-caller/src/write_channel.rs`

```rust
pub fn spawn_write_loop<M, F, Fut>(
    handler: F,
    budget: Option<Arc<Mutex<StreamBudgetController>>>,  // NEW
) -> (mpsc::UnboundedSender<M>, tokio::task::JoinHandle<()>)
```

After each successful `handler(msg).await`, call `replenish` + `flush_pending`:

```rust
let handle = tokio::spawn(async move {
    while let Some(msg) = rx.recv().await {
        match handler(msg).await {
            Ok(written) => {
                if let Some(ref budget) = budget {
                    let mut ctrl = budget.lock().unwrap();
                    ctrl.replenish(written);
                    ctrl.flush_pending(); // calls on_emit for now-eligible streams
                }
            }
            Err(_) => break,
        }
    }
});
```

**This requires the handler to return the number of bytes actually written.** Current handlers return `Result<(), ()>`. Change to `Result<usize, ()>`.

### Transport wiring — each transport creates the budget controller

Each transport's connection setup needs to:

1. Create a `StreamBudgetController` with the desired per-stream limit.
2. Pass it into every `RpcDispatcher::call()` invocation (and thus into
   every encoder created).

**WebSocket server** (`extensions/muxio-tokio-rpc-server/src/rpc_server.rs`):

```rust
let budget = StreamBudgetController::new(
    PER_STREAM_BUDGET,  // e.g. 65536
    Box::new(move |chunk: &[u8]| {
        let _ = tx_clone.send(Message::Binary(Bytes::copy_from_slice(chunk)));
    }),
);
// pass budget into get_emit_fn / dispatcher.call()
```

**WebSocket client**, **IPC server**, **IPC client** follow the same pattern.

---

## TCP / UDP Transport Considerations

### TCP (stream-oriented)

TCP is already handled by the existing WebSocket and IPC transports, which
both use `TcpStream` under the hood (WS via tokio-tungstenite, IPC via Unix
domain sockets / named pipes).  The write loop does `socket.write_all(&msg)`,
which returns the number of bytes written.  This maps cleanly to
`replenish(bytes_written)`.

**Key point:** TCP's flow control is between the local and remote kernels.
The per-stream budget adds a layer of flow control *above* TCP — it limits
how much data the application queues into the kernel's send buffer per
stream.  Without it, a fast producer on stream A can fill the entire kernel
send buffer with that stream's data, delaying stream B's frames.

Implementation for a hypothetical raw TCP transport:

```rust
// write loop
let n = socket.write(&msg).await?; // partial writes possible
ctrl.replenish(n);
ctrl.flush_pending();
```

TCP can do partial writes (`write` returns `< msg.len()`).  `write_all`
hides this.  The budget controller should handle partial writes correctly
(credit only what was actually consumed by the kernel).

### UDP (datagram-oriented)

UDP has no inherent flow control — you'd typically use a pacing mechanism
or application-level ACKs.  The `FrameStreamEncoder` and decoder already
have TODOs for UDP:

```
core/src/frame/frame_stream_encoder.rs:4  // TODO: Add optional `UDP mode`
core/src/frame/frame_mux_stream_decoder.rs:5 // TODO: Add optional `UDP mode`
```

For UDP, the write path would be:

1. `FrameStreamEncoder::emit_frame` encodes a complete datagram (or uses
   `max_chunk_size` to fit in a single MTU).
2. The `on_emit` callback sends the datagram via `socket.send_to()`.
3. The async write loop deals with datagram loss: the budget replenishment
   would come from remote ACKs (you don't know if a datagram was received
   until the remote tells you).

**Budget semantics for UDP:**
- Best-effort: budget is a pacing allowance.  `replenish` is driven by a
  timer or ACK feedback rather than socket write completion.
- Reliable-UDP-on-top (e.g. QUIC-like): the remote sends ACK frames that
  map to per-stream credits (like HTTP/2 WINDOW_UPDATE).  The budget
  controller gets replenished when ACKs arrive.

For an initial implementation, UDP support means:
- The transport write loop reports bytes written to the kernel send buffer.
- The budget controller doesn't distinguish TCP vs UDP — it's just byte
  counting.  The difference is in how `replenish` is called (socket write
  completion for TCP, ACK arrival for reliable UDP).
- The "UDP mode" TODOs in the encoder/decoder would be a separate future
  concern (ensuring frames fit in MTU, handling loss/reordering).

### WASM bridge

WASM writes through the JS bridge (`static_transport_bridge.rs`).  The
`on_emit` sends bytes through `#[wasm_bindgen]` to JS.  Budget works
identically — the WASM callback reports bytes transferred to the JS
buffer, and the async JS event loop calls replenish when the WebSocket
actually flushes.

---

## Budget Policy & Fairness

### Per-stream vs shared budget

Two schools:

| Approach | Tradeoff |
|---|---|
| **Each stream = fixed budget** (e.g. 64KB) | Simple.  N streams = N × 64KB max pending per connection.  Fair — a slow stream can't starve others. |
| **Shared connection budget** (e.g. 1MB ÷ active streams) | More efficient memory use under many concurrent streams.  Requires tracking stream activity and redistributing. |

Recommendation: start with **per-stream fixed budget**, configurable at the
connection level.  A shared budget can be layered on top later.

### Budget value

Default: **65536** bytes (64KB) per stream.  Rationale:
- HTTP/2 default initial window is 65535 bytes.
- Fits within a reasonable latency × bandwidth product for most local
  and LAN connections.
- WAN applications should configure larger values (via application
  tuning).

### When budget is exhausted

`write_bytes` returns `Ok(0)` — the data stays in the encoder's internal
buffer.  The caller (e.g. the MPSC adapter's background task) should
retry after a yield or when the write loop signals available budget.

Alternatively, add a new error variant `FrameEncodeError::StreamBlocked`
and have the caller wait on a notification channel.

---

## Implementation Order

### Phase 1: Core budget controller (no external behaviour change)

1. Create `core/src/frame/stream_budget.rs` with `StreamBudgetController`.
2. Add `budget: Option<Arc<Mutex<StreamBudgetController>>>` to
   `FrameStreamEncoder` and `RpcStreamEncoder`.
3. Thread through `RpcDispatcher::call()`, `caller_interface.rs`,
   `write_channel.rs`.
4. Wire into existing transports with `budget: None` — no functional
   change, all tests pass.

This is ~400 lines of new code, zero behavioural changes.  Verify by:
```
cargo build && cargo test
```

### Phase 2: Enable budgets on one transport

5. Pick one transport (e.g. WS client), create a
   `StreamBudgetController` with a generous budget (256KB), pass it
   through.
6. The write loop now calls `replenish` + `flush_pending` after each
   socket write.
7. Verify existing streaming tests still pass (budget is large enough
   that no stream blocks in practice).

### Phase 3: Test starvation / fairness

8. Write a test: two concurrent streams, one producer is much faster
   than the other, small per-stream budget (e.g. 1KB).  Verify:
   - Fast stream stalls when it exceeds budget.
   - Slow stream continues to make progress.
   - When fast stream's budget is replenished, it sends its buffered
     data.
9. Tune budget tracking — ensure `flush_pending` doesn't hold the
   mutex for too long (batch frame emission).

### Phase 4: Wire all transports

10. Enable budgets in WS server, IPC client, IPC server — same pattern
    as Phase 2.
11. Verify cross-transport integration tests.

### Phase 5: UDP considerations

12. When a raw UDP transport is added:
    - `on_emit` writes datagrams, not streams.
    - Budget replenishment may need an ACK timer or remote feedback.
    - Modify `StreamBudgetController` to support timer-driven
      replenish for unreliable transports.

---

## Open Questions

1. **`write_bytes` return type** — return `Ok(0)` on stall and let the
   caller retry, or add `FrameEncodeError::StreamBlocked`?  The MPSC
   adapter's background task loops on `req_rx.recv()` — returning `Ok(0)`
   means it keeps consuming from the unbounded channel, piling up data
   in the encoder buffer.  Better to return `Err(StreamBlocked)` and
   block the receiver until budget is available.

2. **Notification mechanism** — when `replenish` makes pending frames
   eligible, how does the stalled producer know to retry?  Options:
   - Polling on next `write_bytes` call (simple, may introduce latency).
   - `tokio::sync::Notify` per stream (more responsive, more complex).
   - The MPSC adapter's background task re-checks after a `tokio::task::yield_now()`.

3. **WINDOW_UPDATE frames** — for remote flow control (not just local
   channel backpressure), we'd need a new `FrameKind::WindowUpdate`
   carrying a `stream_id` + `increment`.  The decoder would call
   `StreamBudgetController::replenish` on receipt.  Out of scope for
   the initial fix but would make Muxio fully self-regulating across
   the network.

4. **Budget and the MPSC adapter** — the MPSC adapter's unbounded
   `req_tx`/`req_rx` channel can still accumulate data in front of the
   encoder even with budgets enabled.  Should the adapter also become
   bounded?  (It would then backpressure the *user*, which the adapter
   was specifically designed to avoid.)

---

## Files touched (summary)

| File | Change |
|---|---|
| `core/src/frame/stream_budget.rs` | **New** — `StreamBudgetController` |
| `core/src/frame/frame_stream_encoder.rs` | Add `budget` field, modify `write_bytes`/`flush` |
| `core/src/rpc/rpc_internals/rpc_stream_encoder.rs` | Thread `budget` in constructor |
| `core/src/rpc/rpc_dispatcher.rs` | Thread `budget` in `call()` |
| `extensions/muxio-rpc-service-caller/src/caller_interface.rs` | Thread `budget` in `call_rpc_streaming` |
| `extensions/muxio-rpc-service-caller/src/write_channel.rs` | `spawn_write_loop` takes budget, calls replenish/flush |
| All 4 transport impls (WS client/server, IPC client/server) | Create `StreamBudgetController`, pass through |
| `extensions/muxio-mpsc-adapter/src/client.rs` | Optionally thread budget through `open_channel` |
