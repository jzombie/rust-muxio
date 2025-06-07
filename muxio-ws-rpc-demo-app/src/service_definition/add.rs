use bitcode::{Decode, Encode};
use muxio::rpc::optional_traits::{RpcRequestPrebuffered, RpcResponsePrebuffered};
use std::io;

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct AddRequestParams {
    pub numbers: Vec<f64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct AddResponseParams {
    pub result: f64,
}

pub struct Add;

impl RpcRequestPrebuffered for Add {
    const METHOD_ID: u64 = 0x01;

    type Input = Vec<f64>;
    type DecodedRequest = AddRequestParams;

    fn encode_request(numbers: Vec<f64>) -> Vec<u8> {
        bitcode::encode(&AddRequestParams { numbers })
    }

    fn decode_request(bytes: Vec<u8>) -> Result<AddRequestParams, io::Error> {
        let result = bitcode::decode::<AddRequestParams>(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(result)
    }
}

impl RpcResponsePrebuffered for Add {
    const METHOD_ID: u64 = 0x01;

    type Output = f64;
    type DecodedResponse = f64;

    fn encode_response(result: f64) -> Vec<u8> {
        bitcode::encode(&AddResponseParams { result })
    }

    fn decode_response(bytes: Vec<u8>) -> Result<f64, io::Error> {
        let raw = bitcode::decode::<AddResponseParams>(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(raw.result)
    }
}
