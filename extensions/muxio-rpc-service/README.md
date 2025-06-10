# muxio-rpc-service

Implements a transport-agnostic RPC service interface with support for both pre-buffered and streaming call semantics. It defines traits for encoding and decoding RPC method calls (`RpcMethodPrebuffered`), and provides a unified interface (`RpcClientInterface`) for sending requests and receiving responses over arbitrary transports.
