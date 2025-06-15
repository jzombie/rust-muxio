# Muxio: A High-Performance Multiplexing and RPC Framework for Rust

**DRAFT -- WORK IN PROGRESS**

Muxio provides a robust and flexible foundation for building high-performance, transport-agnostic, and runtime-agnostic services in Rust. It offers a layered architecture that cleanly separates low-level binary stream multiplexing from high-level RPC logic, enabling you to create custom-tailored communication protocols.

## What is Muxio?

At its core, Muxio is a set of layered components that enable multiplexed data transmission over a single, unified connection. Think of it as a toolkit for managing multiple, independent data streams, such as RPC calls, file transfers, and real-time data feeds—without the overhead of multiple connections.

On top of this multiplexing layer, Muxio offers a minimal, unopinionated RPC framework. While you can use it directly, it's often more practical to leverage the provided extensions, which offer ready-to-use solutions for common environments (such as `tokio` or `WASM`).

----------

## Key Features

- **Efficient Multiplexing**: Muxio's foundational framing protocol can reliably manage numerous concurrent data streams over a single connection, correctly reassembling interleaved and out-of-order frames.

- **Minimalist RPC Layer**: A lightweight RPC mechanism is provided on top of the framing layer, giving you the freedom to choose your own serialization formats, dispatching logic, and error-handling strategies.

- **Low-Overhead Binary Protocol**: Muxio uses a compact binary framing protocol to minimize data transmission overhead, making it highly efficient for performance-sensitive applications. All communication, from frame headers to RPC payloads, is handled as raw bytes. The protocol defines a minimal header structure to keep data transfer lean.

- **Transport and Runtime Agnostic**: The core logic uses a flexible, callback-driven design, enabling seamless adaptation across different environments. It supports both Tokio and standard library servers, as well as native and WASM clients, with or without Tokio.

- **Extensible by Design:** Muxio comes with pre-built extensions that demonstrate how to integrate the core library into real-world applications:.

  - **Tokio-based [Server](./extensions/muxio-tokio-rpc-server)/[Client](./extensions/muxio-tokio-rpc-client)]**: For native, multi-threaded environments.
  - **[WASM-based Web Client](./extensions/muxio-wasm-rpc-client)**: For seamless integration into web applications, communicating with a JavaScript host via a simple byte-passing bridge.

TODO: Mention client/server abstractions (callers & endpoints)




######################

--- TODO: Replace below --- 

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
