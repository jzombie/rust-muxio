use bitcode::{Decode, Encode};
use muxio_rpc_service::{RpcMethodPrebuffered, rpc_method_id};
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
    const METHOD_ID: u64 = rpc_method_id!("math.add");

    type Input = Vec<f64>;
    type Output = f64;

    fn encode_request(numbers: Self::Input) -> Result<Vec<u8>, io::Error> {
        Ok(bitcode::encode(&AddRequestParams { numbers }))
    }

    fn decode_request(bytes: Vec<u8>) -> Result<Self::Input, io::Error> {
        let req_params = bitcode::decode::<AddRequestParams>(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(req_params.numbers)
    }

    fn encode_response(result: Self::Output) -> Result<Vec<u8>, io::Error> {
        Ok(bitcode::encode(&AddResponseParams { result }))
    }

    fn decode_response(bytes: Vec<u8>) -> Result<Self::Output, io::Error> {
        let resp_params = bitcode::decode::<AddResponseParams>(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(resp_params.result)
    }
}
