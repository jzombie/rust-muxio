# Naming Conventions

## RPC Types (`core/src/rpc/`)

All RPC-related structs, traits, enums, and type aliases use the `Rpc` prefix:

- **Structs:** `RpcDispatcher`, `RpcSession`, `RpcRequest`, `RpcResponse`, `RpcHeader`, `RpcRespondableSession`, `RpcStreamEncoder`, `RpcStreamDecoder`
- **Traits:** `RpcEmit`, `RpcResponseHandler`, `RpcStreamEventDecoderHandler`
- **Enums:** `RpcMessageType`, `RpcStreamEvent`
- **Type aliases:** `RpcResponseWriter`, `RpcResponseBuffer`, `RpcStreamMethodRouter` — defined in `core/src/rpc/rpc_internals/rpc_trait.rs`

Canonical type aliases live in `rpc_trait.rs` and are re-used across crates rather than duplicated.

## Frame Types (`core/src/frame/`)

Frame-level types use the `Frame` prefix:
- `FrameCodec`, `FrameKind`, `FrameDecodeError`, `FrameEncodeError`, `FrameMuxStreamDecoder`, `FrameStreamEncoder`, `Frame`

## Fields

All fields on RPC structs use an `rpc_` prefix:
- `rpc_method_id`, `rpc_request_id`, `rpc_param_bytes`, `rpc_prebuffered_payload_bytes`, `rpc_result_status`, `rpc_msg_type`, `rpc_metadata_bytes`

## Files

Module files in `core/src/rpc/` use an `rpc_` prefix:
- `rpc_dispatcher.rs`, `rpc_session.rs`, `rpc_stream_encoder.rs`, `rpc_stream_decoder.rs`, `rpc_trait.rs`, etc.

Module files in `core/src/frame/` use a `frame_` prefix:
- `frame_codec.rs`, `frame_kind.rs`, `frame_stream_encoder.rs`, etc.

Extension crate source files use unprefixed names:
- `rpc_client.rs`, `rpc_server.rs`, `endpoint.rs`, `caller_interface.rs`

## Functions

- Free functions: `snake_case` (e.g., `increment_u32_id`, `now`)
- Methods on RPC types: `snake_case` with `rpc_` prefix where applicable (e.g., `from_rpc_header`)

# Cross-Transport Constraints

All transports (ws, ipc, wasm, etc.) must behave identically. Never special-case or exclude any transport from a test, macro, or feature — if a test or feature doesn't work on a transport, fix the underlying issue rather than gating it out.

Rules:
- Every test macro invocation in test files must include all three transports: `ws`, `ipc`, `wasm`
- Never add `#[cfg(not(target_arch = "wasm32"))]` or any transport-excluding conditional compilation
- Never implement a `TestTransport` method as a no-op or panic for a specific transport
- If a transport lacks a capability (e.g., WASM `read_bytes` skipping streaming handler routing), fix the transport — do not gate the test
- If a capability cannot be implemented on a transport (extremely rare, e.g. Unix-specific), the macro must still invoke that transport and the test body must be a compile-time or runtime assertion explaining why
