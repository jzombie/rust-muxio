use bitcode::{Decode, Encode};

pub trait RpcApi {
    const METHOD_ID: u64;

    type Input;
    type EncodedRequest;
    type DecodedRequest;

    type Output;
    type EncodedResponse;
    type DecodedResponse;

    fn encode_request(input: Self::Input) -> Self::EncodedRequest;
    fn decode_request(bytes: Vec<u8>) -> Result<Self::DecodedRequest, bitcode::Error>;

    fn encode_response(output: Self::Output) -> Self::EncodedResponse;
    fn decode_response(bytes: Vec<u8>) -> Result<Self::DecodedResponse, bitcode::Error>;
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

    fn decode_request(bytes: Vec<u8>) -> Result<AddRequestParams, bitcode::Error> {
        bitcode::decode::<AddRequestParams>(&bytes)
    }

    fn encode_response(result: f64) -> Vec<u8> {
        bitcode::encode(&AddResponseParams { result })
    }

    fn decode_response(bytes: Vec<u8>) -> Result<f64, bitcode::Error> {
        let raw = bitcode::decode::<AddResponseParams>(&bytes)?;

        Ok(raw.result)
    }
}
