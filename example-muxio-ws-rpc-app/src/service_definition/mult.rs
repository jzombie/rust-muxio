use bitcode::{Decode, Encode};
use muxio_service_traits::{RpcRequestPrebuffered, RpcResponsePrebuffered};
use std::io;

const MULT_METHOD_ID: u64 = 0x02;

#[derive(Encode, Decode, PartialEq, Debug)]
struct MultRequestParams {
    pub numbers: Vec<f64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
struct MultResponseParams {
    pub result: f64,
}

pub struct Mult;

impl RpcRequestPrebuffered for Mult {
    const METHOD_ID: u64 = MULT_METHOD_ID;

    type Input = Vec<f64>;

    fn encode_request(numbers: Self::Input) -> Vec<u8> {
        bitcode::encode(&MultRequestParams { numbers })
    }

    fn decode_request(bytes: Vec<u8>) -> Result<Self::Input, io::Error> {
        let raw = bitcode::decode::<MultRequestParams>(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(raw.numbers)
    }
}

impl RpcResponsePrebuffered for Mult {
    const METHOD_ID: u64 = MULT_METHOD_ID;

    type Output = f64;

    fn encode_response(result: Self::Output) -> Vec<u8> {
        bitcode::encode(&MultResponseParams { result })
    }

    fn decode_response(bytes: Vec<u8>) -> Result<Self::Output, io::Error> {
        let raw = bitcode::decode::<MultResponseParams>(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(raw.result)
    }
}
