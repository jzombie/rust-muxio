# Muxio: An Extensible Framework for Multiplexed, Schemaless RPC

**DRAFT -- WORK IN PROGRESS**

Muxio is a high-performance, asynchronous RPC and stream multiplexing framework for Rust. It provides a set of foundational components for building robust, transport-agnostic, and runtime-agnostic services.

At its core, Muxio is designed with a layered architecture that separates the low-level framing protocol from the higher-level RPC logic, giving you the flexibility to build custom services without being locked into a specific design.

## Key Features

- **Multiplexed Framing Layer**: The base of Muxio is a powerful framing protocol that supports multiple, independent, and concurrent data streams over a single connection. It correctly reassembles interleaved and out-of-order frames.

- **Primitive & Unopinionated RPC Layer**: On top of the framing layer sits a lightweight RPC mechanism. It provides the ability to send requests with metadata and a payload, and receive responses, without imposing strict design patterns.

- **Callback-Driven Core**: The core logic is runtime-agnostic and uses a flexible callback-based approach, making it adaptable to any execution environment.

- **Extensible by Design**: Muxio comes with pre-built extensions that demonstrate how to integrate the core library into real-world applications:

  - **Tokio-based Server/Client**: For native, multi-threaded environments.

  - **WASM Client**: For seamless integration into web applications.

## Core Concepts

Muxio is built on a two-layer architecture:

1. **Framing Layer** (`muxio::frame`)

This is the foundation of the library. It is responsible for taking a stream of bytes and breaking it into frames, each with a header containing a stream_id and seq_id.

The `FrameMuxStreamDecoder` is the key component here. It can process a raw byte stream from any source (like a TCP or WebSocket connection) and will correctly reassemble frames that arrive interleaved or out of order, buffering them until they can be processed sequentially.

2. **RPC Layer** (`muxio::rpc`)

This layer provides the fundamental building blocks for RPC communication. It uses the framing protocol to transmit structured messages: RpcRequest and RpcResponse. This layer is intentionally minimal, giving you control over serialization, method dispatch, and error handling.
