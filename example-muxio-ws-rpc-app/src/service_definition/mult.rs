use bitcode::{Decode, Encode};
use muxio_rpc_service::{RpcMethodPrebuffered, rpc_method_id};
use std::io;

#[derive(Encode, Decode, PartialEq, Debug)]
struct MultRequestParams {
    pub numbers: Vec<f64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
struct MultResponseParams {
    pub result: f64,
}

pub struct Mult;

impl RpcMethodPrebuffered for Mult {
    const METHOD_ID: u64 = rpc_method_id!("math.mult");

    type Input = Vec<f64>;
    type Output = f64;

    fn encode_request(numbers: Self::Input) -> Result<Vec<u8>, io::Error> {
        Ok(bitcode::encode(&MultRequestParams { numbers }))
    }

    fn decode_request(bytes: &[u8]) -> Result<Self::Input, io::Error> {
        let req_params: MultRequestParams = bitcode::decode::<MultRequestParams>(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(req_params.numbers)
    }

    fn encode_response(result: Self::Output) -> Result<Vec<u8>, io::Error> {
        Ok(bitcode::encode(&MultResponseParams { result }))
    }

    fn decode_response(bytes: &[u8]) -> Result<Self::Output, io::Error> {
        let resp_params = bitcode::decode::<MultResponseParams>(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(resp_params.result)
    }
}
