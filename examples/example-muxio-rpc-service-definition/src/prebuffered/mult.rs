use bitcode::{Decode, Encode};
use muxio_rpc_service::{prebuffered::RpcMethodPrebuffered, rpc_method_id};
use std::io;

#[derive(Encode, Decode, PartialEq, Debug)]
struct MultRequestParams {
    pub numbers: Vec<f64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
struct MultResponseParams {
    pub product: f64,
}

pub struct Mult;

impl RpcMethodPrebuffered for Mult {
    const METHOD_ID: u64 = rpc_method_id!("math.mult");

    type Input = Vec<f64>;
    type Output = f64;

    fn encode_request(numbers: Self::Input) -> Result<Vec<u8>, io::Error> {
        Ok(bitcode::encode(&MultRequestParams { numbers }))
    }

    fn decode_request(request_bytes: &[u8]) -> Result<Self::Input, io::Error> {
        let request_params: MultRequestParams = bitcode::decode::<MultRequestParams>(request_bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(request_params.numbers)
    }

    fn encode_response(product: Self::Output) -> Result<Vec<u8>, io::Error> {
        Ok(bitcode::encode(&MultResponseParams { product }))
    }

    fn decode_response(response_bytes: &[u8]) -> Result<Self::Output, io::Error> {
        let response_params = bitcode::decode::<MultResponseParams>(response_bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(response_params.product)
    }
}
