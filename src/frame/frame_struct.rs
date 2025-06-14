use crate::frame::{FrameDecodeError, FrameKind};

/// Represents a single frame of data in the stream.
///
/// A frame is the basic unit of data that is transmitted in the system. It consists
/// of a header with metadata, such as the stream ID, sequence ID, timestamp, and
/// a payload that contains the actual data being sent.
///
/// A single frame is not necessarily a single chunk of data — multiple frames or even
/// portions of frames may be bundled together in an actual data chunk, depending on
/// the transport protocol.
#[derive(Debug)]
pub struct Frame {
    /// Identifies the logical stream or message.
    ///
    /// The `stream_id` is used to distinguish between different streams of data.
    /// Each logical connection or communication channel has a unique `stream_id`.
    pub stream_id: u32,

    /// The sequence number of the frame within the stream.
    ///
    /// The `seq_id` helps maintain the order of frames within the same stream.
    /// It ensures that the frames are processed in the correct sequence, even if
    /// they arrive out of order.
    pub seq_id: u32,

    /// The type of frame.
    ///
    /// The `kind` field specifies the frame's role in the communication.
    /// Possible values include `Open`, `Data`, `End`, `Cancel`, `Pong`, `Ping`,
    /// which define whether the frame is part of an active stream, the data itself,
    /// or control messages like stream termination or cancellation.
    pub kind: FrameKind, // Open, Data, End, etc.

    /// The timestamp when the frame was sent, in microseconds since the UNIX epoch.
    ///
    /// The `timestamp_micros` provides the time when the frame was generated or sent.
    /// It is used for various purposes such as measuring latency or sequencing frames
    /// that have been generated in different time windows.
    pub timestamp_micros: u64, // Local send timestamp

    /// The raw payload data of the frame.
    ///
    /// The `payload` is the actual data being transmitted in the frame. It is represented
    /// as a vector of bytes, and its interpretation is left to the application layer.
    /// This could contain any kind of application-specific data.
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub struct DecodedFrame {
    pub inner: Frame,
    pub decode_error: Option<FrameDecodeError>,
}
