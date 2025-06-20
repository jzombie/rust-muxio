TODO: Add example server/client in `examples` directory and show how information can be exchanged over multiple terminals (perhaps include an `echo` route and make a demo showing how typing in one terminal can update the other terminal in real-time)

TODO: Mention something about "layered transport kit"

TODO: Ensure reading and writing is standardized to: `read_bytes` and `write_bytes`

TODO: Use `tracing` for logging: tracing = { version = "x", default-features = false, features = ["release_max_level_info"] }

# Core Design Goals

- Binary, not JSON — zero assumptions about serialization (CBOR, f32, FlatBuffers, etc.)

- Framed transport — discrete, ordered binary chunks

- Bidirectional — client/server symmetry (both can send/receive)

- WASM-compatible

- Streamable — supports chunked payloads

- Cancelable — cancel by ID midstream

- Metrics-capable — latency, jitter, throughput if needed

- Client/Server agnostic (client should be able to have the same functionality a server has)

## Test Coverage 

```sh
cargo llvm-cov --summary-only --workspace
```

## Graph Modules

https://github.com/regexident/cargo-modules?tab=readme-ov-file

```sh
cargo modules dependencies --no-externs --no-fns --no-sysroot --no-traits --no-types --no-uses > mods.dot
dot -Tsvg mods.dot -o mods.svg
```

## Release

```sh
 cargo release --workspace 0.5.0-alpha --dry-run
```

## Runtime Model (Draft)

This library is written in a non-async style, using synchronous control flow with callbacks. Despite not depending on async/await, it supports streaming, interleaving, and cancellation through a layered transport kit design. This enables integration with both single-threaded and multi-threaded runtimes, including compatibility with WASM environments where true async tasks may be limited.

  > The runtime model prioritizes low-overhead, deterministic execution, where events are driven by explicit invocations (e.g., ]TODO: List examples]), and callbacks handle downstream I/O or response logic. This architecture supports efficient integration with event-driven systems without imposing runtime-specific constraints.

## Detect Unused Packages

Install prerequisites:

```sh
rustup install nightly
cargo install cargo-udeps
```

Run it on the full workspace:

```sh
cargo +nightly udeps --workspace
```
