TODO: Mention something about "layered transport kit"

# Core Design Goals

- Binary, not JSON — zero assumptions about serialization (CBOR, f32, FlatBuffers, etc.)

- Framed transport — discrete, ordered binary chunks

- Bidirectional — client/server symmetry (both can send/receive)

- WASM-compatible

- Streamable — supports chunked payloads

- Cancelable — cancel by ID midstream

- Metrics-capable — latency, jitter, throughput if needed
