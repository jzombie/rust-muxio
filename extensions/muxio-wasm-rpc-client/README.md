# muxio-wasm-rpc-client

Implements a WebAssembly-compatible RPC client that communicates via a JavaScript-provided socket transport. It provides both a `RpcWasmClient` and static client initialization helpers, handling encoding, dispatching, and response parsing within the browser. Integrates with JavaScript through `wasm_bindgen` for emitting and receiving raw socket frames as `Uint8Array` type.
