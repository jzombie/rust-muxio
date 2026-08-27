# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/) and this project adheres to
(or is loosely based on) Semantic Versioning.

## [Unreleased]

### Added

- **Frame `Transport` decode error variant (`muxio-core`):** new `FrameDecodeError::Transport(String)` carries the real `std::io::Error` display for transport failures so `fail_all_pending_requests` can surface `ConnectionReset`, `BrokenPipe`, or `UnexpectedEof` instead of a static `ReadAfterCancel`.

### Fixed

- **IPC client/server transport error propagation:** client read loop now distinguishes `Ok(0)` (synthesized unexpected EOF) from `Err(e)` (logs `warn!` and stores the error) instead of swallowing via `ok()?`, server read loop does the same, and both shutdown paths thread the concrete error into `fail_all_pending_requests` so pending oneshots receive the root cause.
- **Prebuffering flags leak:** `prebuffering_flags` entries were inserted per outbound call and never removed; now cleared on `End`/`Error` and via `clear_all_prebuffering` in `fail_all`.
- **Inbound request queue leak:** `rpc_request_queue` entries for errored or non-finalized streams lingered; `fail_all_pending_requests` now drains the queue so a dead connection does not leak state.

### Changed

- **Panic-aware server per-connection tasks:** `handle_connection` now retains `writer_handle` and `reader_handle`, checks `JoinError::is_panic()` and logs with `conn_id` instead of silently discarding via `select!` drop, and aborts the peer task.
- **Write queue depth visibility:** defined `WRITE_QUEUE_WARN_THRESHOLD` and `SERVER_WRITE_QUEUE_WARN_THRESHOLD` with a client-side atomic counter and `warn!` when the threshold is exceeded; full backpressure redesign remains deferred.
- **Dependency bumps (`Cargo.lock`):** `futures 0.3.33 → 0.3.34` (#105), `async-trait 0.1.91 → 0.1.92` (#104) — patch bumps via Dependabot (`futures 0.3.34` upgrades `futures-channel`, `futures-core`, `futures-executor`, `futures-io`, `futures-sink`, `futures-task`, `futures-util`, `futures-macro` with `syn 2.0.118 → 3.0.3`).

## [0.15.0-alpha] - 2026-08-19

### Changed

- **BREAKING: Frame header `timestamp_micros` removed (`muxio-core`):** `Frame` no longer
  carries `u64 timestamp_micros` (`core/src/frame/frame_struct.rs:40`,
  `core/src/constants.rs:7` `FRAME_HEADER_SIZE 21 → 13`, `core/src/frame/frame_codec.rs`,
  `core/src/frame/frame_stream_encoder.rs`). Wire is incompatible with `≤0.14.0-alpha` —
  old peers sending 21-byte headers will be rejected as `CorruptFrame`. Saves 8 bytes per
  chunk (~38% header) and removes the `chrono`/`utils::now` dependency (`core/Cargo.toml:12`,
  `core/src/utils/now.rs` deleted, `tests/utils_tests.rs` trimmed). `seq_id` + `stream_id`
  already provide ordering/reassembly (`FrameMuxStreamDecoder`); timestamp was unused beyond
  encode/decode and is not required for reliable delivery, including future UDP mode (which
  will use `seq_id` ACKs). Re-add as optional extension if latency metrics are needed.
- **RPC transport tracing lowered from `DEBUG` to `TRACE` (`muxio-core`):** the per-request
  `RpcDispatcher::init_catch_all_response_handler` spam (`Added request {} to queue`,
  `Appended {} bytes to payload`, `Payload chunk for unknown request`, `Request {} finalized`,
  `End event for unknown request`) now emits at `TRACE` with
  `target: "muxio_rpc_service::transport"` and structured fields (`id`, `bytes`).
  Previously every `Header`/`PayloadChunk`/`End` `RpcStreamEvent` logged at `DEBUG`, hammering
  downstream `LevelFilter::DEBUG` consumers (e.g. `term-wm`'s in-app Debug Log at `~50–150 ms`,
  evicting its 2000-line buffer in ~10 s). Now hidden under the default `DEBUG` filter;
  re-enable with `RUST_LOG=muxio_rpc_service::transport=trace` or `EnvFilter::new("trace")`.
  A `const TRANSPORT_TARGET` is defined once and reused.
- **Transitive dependency bumps (`Cargo.lock`):** `tokio 1.52.3 → 1.53.1`,
  `tokio-tungstenite 0.29.0 → 0.30.0` (with `tungstenite 0.30.0`, `sha1 0.11.0`,
  `data-encoding`), `interprocess 2.4.2 → 2.4.3`, `async-trait 0.1.89 → 0.1.91`,
  `xxhash-rust 0.8.17 → 0.8.18`, plus new `block-buffer 0.12.1`, `crypto-common 0.2.2`,
  `digest 0.11.3`, `const-oid 0.10.2`, `hybrid-array 0.4.14` pulled via the `tungstenite`
  upgrade (`Cargo.toml` bumps `tokio-tungstenite`).
- **Removed unused `tokio-tungstenite` dep from `muxio-tokio-rpc-server`** (`extensions/muxio-tokio-rpc-server/Cargo.toml:21`) — server now uses `axum::extract::ws` (which wraps `tungstenite` transitively); `cargo-udeps` no longer flags it. Client retains `tokio-tungstenite` for `connect_async`.

## [0.14.0-alpha] - 2026-07-31

### Fixed

- **Hung streams on disconnect and request/stream ID collisions (#96):** IPC client only
  failed pending streams if the disconnected flag had not already flipped, and
  `muxio-tokio-mpsc-adapter` dropped `End`/`Error` on a poisoned sender lock — readers
  hung forever. Now always marks transport disconnected and calls
  `fail_all_pending_requests()` on shutdown, recovers from a poisoned lock to ensure
  `End`/`Error` fires, and runs endpoint handlers outside the dispatcher lock
  (`decode_bytes` / `run_handlers` / `send_responses` split) to avoid deadlock when a
  handler re-enters the dispatcher. Includes cross-transport test that an open stream
  terminates on server disconnect.
- **ID space collision (`IdSpace`):** both ends allocated from a process-global counter,
  so a server-initiated call could clobber a client stream handler (e.g. `2147483684+`
  overwriting a client entry). New `IdSpace` type (`core/src/utils/id_space.rs`) reserves
  the high bit as a direction marker — client `0x0000_0000`, server `0x8000_0000`.
  `RpcDispatcher`, `RpcRespondableSession`, and `RpcSession` now take an `IdSpace`;
  WS/IPC servers construct with `IdSpace::Server`. Wire IDs with the high bit set are
  server-initiated.
- **Dependency bumps in #96:** `xxhash-rust 0.8.15 → 0.8.17 (#94)`, `bytemuck 1.25.0 → 1.25.2 (#93)`,
  `futures 0.3.32 → 0.3.33 (#92)`, `bytes 1.12.0 → 1.12.1 (#90)`, `rand 0.10.1 → 0.10.2 (#88)`.
  Also `chrono 0.4.44 → 0.4.45 (#63)`, `tokio 1.50.0 → 1.51.0 (#54)`.

### Added

- **Streaming interface and `muxio-tokio-mpsc-adapter` (#81):** new streaming
  `RpcStreamEvent` handling and mpsc adapter extension.
- **IPC transport, unified transport tests/utils, extracted `muxio-core` (#74):**
  `interprocess`-based IPC transport, shared test harness, and extraction of the
  `muxio-core` crate.
- **Service caller transport state detection (#32), `Pong` delivery fix (#33),**
  **parameterized `host`/`port` args (#34), GitHub docs workflow (#45),**
  **Dependabot for Cargo (#44).**

### Changed

- Cumulative changes from `0.6.0-alpha` / `0.10.0-alpha` → `0.14.0-alpha` (see
  `git log v0.6.0-alpha..fa650ce`): dep bumps `05112026 (#62)`, `(#53)`, badge alt
  text (#51), Coveralls link fix (#78), comment cleanup (#86), plan cleanup (#87), etc.
- Prior alphas before `0.6.0-alpha` not individually documented — see `git log`.
