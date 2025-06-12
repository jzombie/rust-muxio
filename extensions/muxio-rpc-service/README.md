# muxio-rpc-service

TODO: This information is out of date.

Implements a transport-agnostic RPC service interface with support for both pre-buffered and streaming call semantics. It defines traits for encoding and decoding RPC method calls (`RpcMethodPrebuffered`), and provides a unified interface (`RpcServiceCallerInterface`) for sending requests and receiving responses over arbitrary transports.
