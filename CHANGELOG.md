# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/) and this project adheres to
(or is loosely based on) Semantic Versioning.

## [0.14.1-alpha] - 2026-08-19

### Changed

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
