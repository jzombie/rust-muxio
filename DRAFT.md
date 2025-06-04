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