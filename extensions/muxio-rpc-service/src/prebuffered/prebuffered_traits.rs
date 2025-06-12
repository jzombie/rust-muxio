use std::io;

pub trait RpcMethodPrebuffered {
    /// A unique identifier for the RPC method.
    const METHOD_ID: u64; // TODO: Make helper for this to help avoid numeric collisions

    /// The high-level input type expected by the request encoder (e.g., `Vec<f64>`).
    type Input;

    /// The high-level output type returned from the response encoder (e.g., `f64`).
    type Output;

    /// Encodes the request into a byte array.
    fn encode_request(input: Self::Input) -> Result<Vec<u8>, io::Error>;

    /// Decodes raw request bytes into a typed request struct.
    ///
    /// # Arguments
    /// * `bytes` - Serialized request payload.
    fn decode_request(bytes: &[u8]) -> Result<Self::Input, io::Error>;

    /// Encodes the response value into a byte array.
    fn encode_response(output: Self::Output) -> Result<Vec<u8>, io::Error>;

    /// Decodes raw response bytes into a typed response struct or value.
    ///
    /// # Arguments
    /// * `bytes` - Serialized response payload.
    fn decode_response(bytes: &[u8]) -> Result<Self::Output, io::Error>;
}
