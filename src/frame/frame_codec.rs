use crate::{
    constants::{
        FRAME_HEADER_SIZE, FRAME_KIND_OFFSET, FRAME_LENGTH_FIELD_SIZE, FRAME_SEQ_ID_OFFSET,
        FRAME_STREAM_ID_OFFSET, FRAME_TIMESTAMP_OFFSET,
    },
    frame::{DecodedFrame, Frame, FrameDecodeError, FrameKind},
};

/// Provides encoding and decoding functionality for frames.
///
/// The `FrameCodec` is responsible for serializing a `Frame` into a byte stream and
/// deserializing a byte stream back into a `Frame`. It handles the creation and parsing
/// of the frame header, ensuring the correct encoding of all frame fields such as stream ID,
/// sequence ID, kind, timestamp, and payload.
///
/// This is used to package and unpack frames for transmission across a stream, ensuring
/// that all necessary metadata is included alongside the actual payload.
pub struct FrameCodec;

impl FrameCodec {
    /// Encodes a `Frame` into a byte vector.
    ///
    /// This method serializes the `Frame`'s metadata and payload into a byte stream.
    /// The resulting vector can be transmitted over the stream or saved to a buffer.
    ///
    /// # Arguments
    ///
    /// * `frame` - The `Frame` to be encoded.
    ///
    /// # Returns
    ///
    /// Returns a vector of bytes that represents the encoded frame. The frame consists
    /// of the frame's length, stream ID, sequence ID, kind, timestamp, and payload.
    pub fn encode(frame: &Frame) -> Vec<u8> {
        let mut buf = Vec::with_capacity(FRAME_HEADER_SIZE + frame.payload.len());

        // Add the frame length (payload length in bytes)
        buf.extend(&(frame.payload.len() as u32).to_le_bytes());

        // Add the stream ID, sequence ID, frame kind, timestamp, and payload
        buf.extend(&frame.stream_id.to_le_bytes());
        buf.extend(&frame.seq_id.to_le_bytes());
        buf.push(frame.kind as u8);
        buf.extend(&frame.timestamp_micros.to_le_bytes());
        buf.extend(&frame.payload);

        buf
    }

    /// Decodes a byte slice into a `Frame`.
    ///
    /// This method takes a byte buffer representing a serialized frame and attempts to parse it
    /// back into a `Frame` struct. It checks the integrity of the frame, ensuring that the buffer
    /// contains enough data and that all fields can be correctly interpreted.
    ///
    /// # Arguments
    ///
    /// * `buf` - A byte slice representing the frame to decode.
    ///
    /// # Returns
    ///
    /// Returns a `Result` where:
    /// - `Ok(Frame)` contains the decoded `Frame` object.
    /// - `Err(FrameStreamError)` contains an error if the frame is corrupted or malformed.
    ///
    /// The method will return an error if the buffer is too short or if the frame data does not
    /// conform to the expected structure.
    pub fn decode(buf: &[u8]) -> Result<DecodedFrame, FrameDecodeError> {
        if buf.len() < FRAME_HEADER_SIZE {
            return Err(FrameDecodeError::IncompleteHeader); // Not enough data to form a valid frame
        }

        // TODO: Don't use unwrap
        // Extract the length of the payload
        let len = u32::from_le_bytes(buf[0..FRAME_LENGTH_FIELD_SIZE].try_into().unwrap()) as usize;

        // Ensure the buffer contains enough data for the frame (header + payload)
        if buf.len() < FRAME_HEADER_SIZE + len {
            return Err(FrameDecodeError::IncompleteHeader); // Frame size mismatch
        }

        // Parse the stream ID, sequence ID, frame kind, and timestamp
        let stream_id = u32::from_le_bytes(
            buf[FRAME_STREAM_ID_OFFSET..FRAME_SEQ_ID_OFFSET]
                .try_into()
                // TODO: Don't use unwrap
                .unwrap(),
        );
        let seq_id = u32::from_le_bytes(
            buf[FRAME_SEQ_ID_OFFSET..FRAME_KIND_OFFSET]
                .try_into()
                // TODO: Don't use unwrap
                .unwrap(),
        );
        let kind = FrameKind::try_from(buf[FRAME_KIND_OFFSET])
            .map_err(|_| FrameDecodeError::CorruptFrame)?; // Map error to FrameStreamError

        // Extract the timestamp and payload
        let timestamp = u64::from_le_bytes(
            buf[FRAME_TIMESTAMP_OFFSET..FRAME_HEADER_SIZE]
                .try_into()
                // TODO: Don't use unwrap
                .unwrap(),
        );

        // Discard payload if canceled frame
        let payload = match kind {
            FrameKind::Cancel => vec![],
            _ => buf[FRAME_HEADER_SIZE..FRAME_HEADER_SIZE + len].to_vec(),
        };

        let frame = Frame {
            stream_id,
            seq_id,
            kind,
            timestamp_micros: timestamp,
            payload,
        };

        // Return the decoded frame
        let decoded_frame = DecodedFrame {
            inner: frame,
            decode_error: None,
        };

        Ok(decoded_frame)
    }
}
