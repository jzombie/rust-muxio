# `rpc` Directory

The `rpc` directory provides the implementation for Remote Procedure Call (RPC) functionality, including the management of RPC messages, headers, and metadata. It defines the structure and behavior of messages exchanged between clients and servers, providing an abstraction for remote procedure invocation.

## Modules:

* **`rpc_header.rs`**: Defines the `RpcHeader` struct, which contains metadata for an RPC message, such as the message type, message ID, method ID, and metadata. This header is used to encapsulate essential information about the RPC message.

* **`rpc_message_type.rs`**: Defines the `RpcMessageType` enum, which categorizes the types of RPC messages (e.g., `Call`, `Response`, `Event`). These types help identify the purpose of each message and guide how it is processed.

* **`rpc_metadata.rs`**: Defines the `RpcMetadataValue` enum, which represents the different types of metadata that can be included in an RPC message (e.g., `String`, `U32`, `F64`, `Vec<T>`). This module also includes functions for serializing and deserializing metadata.

* **`rpc_session.rs`**: Manages the lifecycle of RPC sessions, including the creation and management of streams, the processing of incoming bytes, and the handling of RPC events (e.g., message reception, stream cancellation).

* **`rpc_stream_decoder.rs`**: Contains the `RpcStreamDecoder` struct, responsible for decoding incoming RPC frames, extracting the message header and payload, and processing the metadata.

* **`rpc_stream_encoder.rs`**: Implements the `RpcStreamEncoder` struct, which encodes RPC messages into frames that can be transmitted over the network. It handles the construction of the header, metadata, and payload.

* **`rpc_stream_event.rs`**: Defines the `RpcStreamEvent` enum, which represents events that occur during the processing of an RPC stream, including header reception, payload chunks, and stream end notifications.

## Key Concepts:

* **RPC Header**: Contains metadata about the RPC message, such as the type of message, the ID, and the method being called. It is essential for routing and interpreting RPC messages.
* **RPC Metadata**: Information included with the RPC message that can vary in type (e.g., strings, integers, booleans, and vectors). It allows flexibility in the data passed between client and server.
* **RPC Stream**: A logical flow of data between a client and server, consisting of frames that include both metadata and payload.
* **Message Types**: Different types of RPC messages, including `Call`, `Response`, and `Event`, each serving a specific purpose in RPC communication.
