use super::RpcApi;
use bitcode::{Decode, Encode};
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
