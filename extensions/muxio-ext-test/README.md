# Muxio Ext Test

This crate is intended for testing of crates that would otherwise have circular dependencies (and therefore not publishable).

## Test discovery

Any Rust files placed under the `tests/` directory (recursively) will be auto-included
when running `cargo test`. A build script (`build.rs`) generates `tests/auto_tests.rs`
which declares modules for each discovered `*.rs` file. Do not edit `tests/auto_tests.rs`
manually; it is regenerated on test builds.

If you need to add tests, create `.rs` files under `tests/` (for example `tests/foo/bar.rs`).
The test runner will generate module names automatically.

## Unified transport integration tests

Three test files exercise every transport against the same set of assertions,
guaranteeing consistent coverage without per-transport duplication:

| Test file | Macros invoked | Tests per transport |
|---|---|---|
| `tests/prebuffered_roundtrip_tests.rs` | `prebuffered_roundtrip_tests!` | 4 (success, error, large payload, method not found) |
| `tests/prebuffered_server_to_client_tests.rs` | `server_to_client_tests!` | 1 (server-initiated echo) |
| `tests/transport_state_tests.rs` | `transport_state_tests!` | 3 (connection failure, state change handler, pending request cancellation) |

### Adding a new transport

1. **Add the client/server crates** to `Cargo.toml` in this crate as `[dependencies]`.
2. **Implement `TestTransport`** for your client type in `src/transports/your_transport.rs`:

    ```rust
    use async_trait::async_trait;
    use muxio_rpc_service_endpoint::RpcServiceEndpoint;
    use std::sync::Arc;
    use crate::test_transport::TestTransport;

    pub struct YourClient;

    #[async_trait]
    impl TestTransport for YourClient {
        type Client = YourClient;
        type S2cHandle = YourServerHandle;  // type that implements RpcServiceCallerInterface

        fn name() -> &'static str { "your_transport" }

        async fn connect() -> (Arc<Self::Client>, Arc<RpcServiceEndpoint<()>>) {
            // Start server, connect client, register standard handlers on
            // the endpoint that processes incoming RPC requests
            // (almost always the **server** endpoint).
            // Also pre-register a 0xBAD handler returning
            // io::Error::other("test error") for the error roundtrip test.
            todo!()
        }

        async fn connect_fail() -> Result<(), std::io::Error> {
            // Connect to nothing — must return an io::Error.
            todo!()
        }

        async fn connect_with_disconnect() -> (Arc<Self::Client>, tokio::sync::oneshot::Sender<()>) {
            // Start a server that accepts one connection, then waits for
            // the returned Sender to be dropped. The server must:
            //   1. Accept & handshake the transport
            //   2. Drive I/O until the sender is dropped
            //   3. Explicitly close the connection
            todo!()
        }

        async fn connect_s2c()
            -> (Arc<Self::Client>, Arc<RpcServiceEndpoint<()>>, Self::S2cHandle)
        {
            // Start server with an event channel, connect client, wait
            // for ClientConnected event, return handle + client + endpoint.
            todo!()
        }
    }
    ```

3. **Register the module** in `src/transports/mod.rs`.
4. **Add one line to each test file** in `tests/`:

    ```rust
    // In tests/prebuffered_roundtrip_tests.rs:
    prebuffered_roundtrip_tests!(your_transport, YourClient);
    // In tests/prebuffered_server_to_client_tests.rs:
    server_to_client_tests!(your_transport, YourClient);
    // In tests/transport_state_tests.rs:
    transport_state_tests!(your_transport, YourClient);
    ```

5. **Run all transport tests:**

    ```shell
    cargo test -p muxio-ext-test
    ```

### Implementation notes

- **Standard handlers** (Add, Mult, Echo) **must** be registered on the endpoint
  that processes incoming RPC requests — for all existing transports this is
  the **server** endpoint. Registering only on the client endpoint will cause
  `RPC method not found` errors because the request bytes travel client →
  transport → server before being dispatched to a handler.
- **`connect_with_disconnect`** must actively drive the transport's I/O
  (not just store the stream) while waiting for the disconnect signal.
  Use a `tokio::select!` loop that polls both the signal and the incoming
  data stream, and explicitly close the connection when the signal fires.
- The `0xBAD` test error handler must return
  `std::io::Error::other("test error")` — the shared error roundtrip test
  asserts that the error message contains `"test error"`.
