use bitcode::{Decode, Encode};

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct AddRequestParams {
    pub numbers: Vec<f64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct AddResponseParams {
    pub result: f64,
}

pub struct Add;

// TODO: Use common trait
impl Add {
    pub const METHOD_ID: u64 = 0x01;

    pub fn encode_request(numbers: Vec<f64>) -> Vec<u8> {
        bitcode::encode(&AddRequestParams { numbers })
    }

    pub fn decode_request(bytes: Vec<u8>) -> Result<AddRequestParams, bitcode::Error> {
        bitcode::decode::<AddRequestParams>(&bytes)
    }

    pub fn encode_response(result: f64) -> Vec<u8> {
        bitcode::encode(&AddResponseParams { result })
    }

    pub fn decode_response(bytes: Vec<u8>) -> Result<f64, bitcode::Error> {
        let raw = bitcode::decode::<AddResponseParams>(&bytes)?;

        Ok(raw.result)
    }
}
