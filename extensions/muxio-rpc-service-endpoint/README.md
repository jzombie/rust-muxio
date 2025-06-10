# muxio-rpc-service-endpoint

Intended as the future location for reusable server-side endpoint logic. It defines an `RpcEndpoint` struct that ties together a dispatcher, a registry of method handlers, and a byte send function, enabling server components to process inbound request bytes and emit appropriate responses.
