TODO: Mention something about "layered transport kit"

# Core Design Goals

- Binary, not JSON — zero assumptions about serialization (CBOR, f32, FlatBuffers, etc.)

- Framed transport — discrete, ordered binary chunks

- Bidirectional — client/server symmetry (both can send/receive)

- WASM-compatible

- Streamable — supports chunked payloads

- Cancelable — cancel by ID midstream

- Metrics-capable — latency, jitter, throughput if needed

## Test Coverage 

```sh
cargo llvm-cov --summary-only
```

## Graph Modules

https://github.com/regexident/cargo-modules?tab=readme-ov-file

```sh
cargo modules dependencies --no-externs --no-fns --no-sysroot --no-traits --no-types --no-uses > mods.dot
dot -Tsvg mods.dot -o mods.svg
```

## Runtime Model (Draft)

This library is written in a non-async style, using synchronous control flow with callbacks. Despite not depending on async/await, it supports streaming, interleaving, and cancellation through a layered transport kit design. This enables integration with both single-threaded and multi-threaded runtimes, including compatibility with WASM environments where true async tasks may be limited.

  > The runtime model prioritizes low-overhead, deterministic execution, where events are driven by explicit invocations (e.g., ]TODO: List examples]), and callbacks handle downstream I/O or response logic. This architecture supports efficient integration with event-driven systems without imposing runtime-specific constraints.
  