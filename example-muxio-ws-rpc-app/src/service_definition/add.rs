use bitcode::{Decode, Encode};
use muxio_service_traits::RpcMethodPrebuffered;
use std::io;

#[derive(Encode, Decode, PartialEq, Debug)]
struct AddRequestParams {
    pub numbers: Vec<f64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
struct AddResponseParams {
    pub result: f64,
}

pub struct Add;

impl RpcMethodPrebuffered for Add {
    const METHOD_ID: u64 = 0x01;

    type Input = Vec<f64>;
    type Output = f64;

    fn encode_request(numbers: Self::Input) -> Vec<u8> {
        bitcode::encode(&AddRequestParams { numbers })
    }

    fn decode_request(bytes: Vec<u8>) -> Result<Self::Input, io::Error> {
        let raw = bitcode::decode::<AddRequestParams>(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(raw.numbers)
    }

    fn encode_response(result: Self::Output) -> Vec<u8> {
        bitcode::encode(&AddResponseParams { result })
    }

    fn decode_response(bytes: Vec<u8>) -> Result<Self::Output, io::Error> {
        let raw = bitcode::decode::<AddResponseParams>(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(raw.result)
    }
}
