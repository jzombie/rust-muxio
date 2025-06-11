# muxio-tokio-rpc-server

Implements a minimal RPC server over WebSocket using `axum`. It sets up a WebSocket route, reads incoming binary RPC messages, dispatches them to registered method handlers, and streams responses back to clients. Serves as a basic reference server implementation for Tokio-based systems.

> Note: The Muxio server is optional. Any transport that moves bytes from point A to point B can be used. The `muxio-rpc-service-endpoint` crate provides a generic interface for processing incoming byte streams and registering RPC handlers. This server is one such implementation. In bidirectional setups, a client may also act as a server.
