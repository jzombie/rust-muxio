# muxio-tokio-rpc-client

Provides an RPC client implementation over a WebSocket transport using `tokio-tungstenite`. It conforms to the `RpcClientInterface` and handles sending encoded RPC calls, receiving binary responses, and streaming them to user-defined handlers. Designed for native async environments.
