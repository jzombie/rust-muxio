use std::io;

pub trait RpcMethodPrebuffered {
    /// A unique identifier for the RPC method.
    ///
    /// Use the [`rpc_method_id!`](crate::rpc_method_id) macro to generate a
    /// deterministic hash from a string literal at compile time. In debug builds,
    /// the macro performs a runtime collision check — if two names produce the same
    /// hash, the program will panic with a clear message identifying the duplicate.
    const METHOD_ID: u64;

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
    fn decode_request(request_bytes: &[u8]) -> Result<Self::Input, io::Error>;

    /// Encodes the response value into a byte array.
    fn encode_response(output: Self::Output) -> Result<Vec<u8>, io::Error>;

    /// Decodes raw response bytes into a typed response struct or value.
    ///
    /// # Arguments
    /// * `bytes` - Serialized response payload.
    fn decode_response(response_bytes: &[u8]) -> Result<Self::Output, io::Error>;
}

// TODO: Integrate
// // Blanket impl for types that use `bitcode` encoding.
// pub trait BitcodeRpcMethodPrebuffered: RpcMethodPrebuffered
// where
//     Self::Input: Encode + for<'de> Decode<'de>,
//     Self::Output: Encode + for<'de> Decode<'de>,
// {
//     fn encode_request(input: Self::Input) -> Result<Vec<u8>, io::Error> {
//         bitcode::encode(&input).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
//     }

//     fn decode_request(request_bytes: &[u8]) -> Result<Self::Input, io::Error> {
//         bitcode::decode(request_bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
//     }

//     fn encode_response(output: Self::Output) -> Result<Vec<u8>, io::Error> {
//         bitcode::encode(&output).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
//     }

//     fn decode_response(response_bytes: &[u8]) -> Result<Self::Output, io::Error> {
//         bitcode::decode(response_bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
//     }
// }

// // Blanket implementation that lifts into the base trait.
// impl<T> RpcMethodPrebuffered for T
// where
//     T: BitcodeRpcMethodPrebuffered,
// {
//     const METHOD_ID: u64 = T::METHOD_ID;

//     type Input = T::Input;
//     type Output = T::Output;

//     fn encode_request(input: Self::Input) -> Result<Vec<u8>, io::Error> {
//         <T as BitcodeRpcMethodPrebuffered>::encode_request(input)
//     }

//     fn decode_request(request_bytes: &[u8]) -> Result<Self::Input, io::Error> {
//         <T as BitcodeRpcMethodPrebuffered>::decode_request(request_bytes)
//     }

//     fn encode_response(output: Self::Output) -> Result<Vec<u8>, io::Error> {
//         <T as BitcodeRpcMethodPrebuffered>::encode_response(output)
//     }

//     fn decode_response(response_bytes: &[u8]) -> Result<Self::Output, io::Error> {
//         <T as BitcodeRpcMethodPrebuffered>::decode_response(response_bytes)
//     }
// }
