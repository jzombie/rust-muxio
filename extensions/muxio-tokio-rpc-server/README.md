# muxio-tokio-rpc-server

Implements a minimal RPC server over WebSocket using `axum`. It sets up a WebSocket route, reads incoming binary RPC messages, dispatches them to registered method handlers, and streams responses back to clients. Serves as a basic reference server implementation for Tokio-based systems.
