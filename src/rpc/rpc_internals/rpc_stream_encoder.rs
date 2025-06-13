use crate::{
    frame::{FrameEncodeError, FrameStreamEncoder},
    rpc::rpc_internals::RpcHeader,
};

pub struct RpcStreamEncoder<F>
where
    F: FnMut(&[u8]),
{
    stream_id: u32,
    encoder: FrameStreamEncoder<F>,
}

impl<F> RpcStreamEncoder<F>
where
    F: FnMut(&[u8]),
{
    pub fn new(
        stream_id: u32,
        max_chunk_size: usize,
        header: &RpcHeader,
        on_emit: F,
    ) -> Result<Self, FrameEncodeError> {
        let mut encoder = FrameStreamEncoder::new(stream_id, max_chunk_size, on_emit);

        let mut meta_buf = Vec::new();
        meta_buf.push(header.rpc_msg_type as u8);
        meta_buf.extend(&header.rpc_request_id.to_le_bytes());
        meta_buf.extend(&header.method_id.to_le_bytes());

        let metadata_bytes = &header.metadata_bytes;

        // Serialize metadata length (u16)
        let meta_len = metadata_bytes.len() as u16;
        meta_buf.extend(&meta_len.to_le_bytes());

        // Add metadata to the buffer
        meta_buf.extend(metadata_bytes);

        encoder.write_bytes(&meta_buf)?;

        Ok(Self { stream_id, encoder })
    }

    pub fn stream_id(&self) -> u32 {
        self.stream_id
    }

    pub fn write_bytes(&mut self, data: &[u8]) -> Result<usize, FrameEncodeError> {
        self.encoder.write_bytes(data)
    }

    pub fn flush(&mut self) -> Result<usize, FrameEncodeError> {
        self.encoder.flush()
    }

    pub fn cancel_stream(&mut self) -> Result<usize, FrameEncodeError> {
        self.encoder.cancel_stream()
    }

    pub fn end_stream(&mut self) -> Result<usize, FrameEncodeError> {
        self.encoder.end_stream()
    }
}
