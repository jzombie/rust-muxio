use bitcode::{Decode, Encode};
use muxio_rpc_service::{prebuffered::RpcMethodPrebuffered, rpc_method_id};
use std::io;

#[derive(Encode, Decode, PartialEq, Debug)]
struct AddRequestParams {
    pub numbers: Vec<f64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
struct AddResponseParams {
    pub sum: f64,
}

pub struct Add;

impl RpcMethodPrebuffered for Add {
    const METHOD_ID: u64 = rpc_method_id!("math.add");

    type Input = Vec<f64>;
    type Output = f64;

    fn encode_request(numbers: Self::Input) -> Result<Vec<u8>, io::Error> {
        Ok(bitcode::encode(&AddRequestParams { numbers }))
    }

    fn decode_request(request_bytes: &[u8]) -> Result<Self::Input, io::Error> {
        let request_params = bitcode::decode::<AddRequestParams>(request_bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(request_params.numbers)
    }

    fn encode_response(sum: Self::Output) -> Result<Vec<u8>, io::Error> {
        Ok(bitcode::encode(&AddResponseParams { sum }))
    }

    fn decode_response(response_bytes: &[u8]) -> Result<Self::Output, io::Error> {
        let response_params = bitcode::decode::<AddResponseParams>(response_bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(response_params.sum)
    }
}
