# `frame` Directory

The `frame` directory handles the encoding, decoding, and processing of data frames in the system. These frames are used to transport messages within **logical streams**, which are distinct from the underlying physical connection or transport layer (such as `TCP` or `UDP`). The modules in this directory manage the construction and interpretation of these frames, ensuring data integrity, sequence ordering, and proper handling of stream control messages (such as stream cancellation and ending).

## Modules:

* **`frame.rs`**: Contains the `Frame` struct and its definition. The `Frame` is used to encapsulate the structure of a message within a **logical stream**, including metadata (e.g., stream ID, sequence ID, timestamp) and the payload (the actual data being transmitted).

* **`frame_codec.rs`**: Implements the encoding and decoding logic for frames. The `FrameCodec` struct handles the serialization and deserialization of frame data, ensuring that frames are correctly constructed and parsed when sent or received.

* **`frame_errors.rs`**: Defines the `FrameStreamError` enum, which represents different types of errors that can occur during frame processing, including corruption and stream cancellation.

* **`frame_kind.rs`**: Defines the `FrameKind` enum, which categorizes different types of frames (e.g., `Open`, `Data`, `End`, `Cancel`). Each frame type has a specific function in the context of the **logical stream**.

* **`frame_mux_stream_decoder.rs`**: Implements the `FrameMuxStreamDecoder` struct, which is responsible for reassembling and decoding frames that are received. It manages the buffering of partial frames and ensures that frames are decoded in order.

* **`frame_stream_encoder.rs`**: Implements the `FrameStreamEncoder` struct, which handles the encoding of frames to be sent over the stream. This module manages the splitting of larger payloads into smaller chunks and ensures proper sequencing.

### Key Concepts:

* **Frames**: Fundamental units of data in the **logical stream**. Each frame includes a header (with metadata like stream ID, sequence ID, and timestamp) and a payload (the actual data being transmitted).

* **Logical Stream**: A sequence of frames identified by a stream ID. These logical streams may be multiplexed over a single physical connection (e.g., `TCP` or `UDP`), with each stream carrying its own sequence of frames.

* **Physical Stream**: The actual underlying transport connection used to transmit data, such as a `TCP` or `UDP` connection. Multiple **logical streams** can be transmitted over the same physical stream.

* **Frame Kinds**: Different types of frames, such as `Open`, `Data`, `End`, `Cancel`, which determine the role and behavior of the frame in the **logical stream**.

* **Encoding and Decoding**: Frames are serialized into byte arrays for transmission over the **physical stream** and deserialized back into structured data when received, ensuring that the logical stream remains consistent.
