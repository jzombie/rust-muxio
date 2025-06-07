use bitcode::{Decode, Encode};
use std::io;

/// A trait to define the contract for an RPC API service.
///
/// This trait allows a service to define its own encoding and decoding logic
/// for both request and response types while preserving its own ergonomic API.
///
/// Implementors are free to define custom input/output types and how they are encoded
/// or decoded, as long as the required methods are provided.
pub trait RpcApi {
    /// A unique identifier for the RPC method.
    const METHOD_ID: u64;

    /// The high-level input type expected by the request encoder (e.g., `Vec<f64>`).
    type Input;

    /// The serialized request payload type (e.g., `Vec<u8>`).
    type EncodedRequest;

    /// The deserialized request struct (e.g., `AddRequestParams`).
    type DecodedRequest;

    /// The high-level output type returned from the response encoder (e.g., `f64`).
    type Output;

    /// The serialized response payload type (e.g., `Vec<u8>`).
    type EncodedResponse;

    /// The deserialized response struct (e.g., `AddResponseParams`).
    type DecodedResponse;

    /// Encodes the user input into a serialized request payload.
    ///
    /// # Arguments
    /// * `input` - The high-level input to encode (e.g., parameters like numbers to add).
    fn encode_request(input: Self::Input) -> Self::EncodedRequest;

    /// Decodes raw request bytes into a typed request struct.
    ///
    /// # Arguments
    /// * `bytes` - Serialized request payload.
    fn decode_request(bytes: Vec<u8>) -> Result<Self::DecodedRequest, io::Error>;

    /// Encodes the response value into a serialized payload.
    ///
    /// # Arguments
    /// * `output` - The high-level response result to encode (e.g., the sum).
    fn encode_response(output: Self::Output) -> Self::EncodedResponse;

    /// Decodes raw response bytes into a typed response struct or value.
    ///
    /// # Arguments
    /// * `bytes` - Serialized response payload.
    fn decode_response(bytes: Vec<u8>) -> Result<Self::DecodedResponse, io::Error>;
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct AddRequestParams {
    pub numbers: Vec<f64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct AddResponseParams {
    pub result: f64,
}

pub struct Add;

impl RpcApi for Add {
    const METHOD_ID: u64 = 0x01;

    type Input = Vec<f64>;
    type EncodedRequest = Vec<u8>;
    type DecodedRequest = AddRequestParams;

    type Output = f64;
    type EncodedResponse = Vec<u8>;
    type DecodedResponse = f64;

    fn encode_request(numbers: Vec<f64>) -> Vec<u8> {
        bitcode::encode(&AddRequestParams { numbers })
    }

    fn decode_request(bytes: Vec<u8>) -> Result<AddRequestParams, io::Error> {
        let result = bitcode::decode::<AddRequestParams>(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(result)
    }

    fn encode_response(result: f64) -> Vec<u8> {
        bitcode::encode(&AddResponseParams { result })
    }

    fn decode_response(bytes: Vec<u8>) -> Result<f64, io::Error> {
        let raw = bitcode::decode::<AddResponseParams>(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(raw.result)
    }
}
