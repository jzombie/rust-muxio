use muxio_rpc_service::{prebuffered::RpcMethodPrebuffered, rpc_method_id};
use std::io;

pub struct Echo;

impl RpcMethodPrebuffered for Echo {
    const METHOD_ID: u64 = rpc_method_id!("echo");

    type Input = Vec<u8>;
    type Output = Vec<u8>;

    fn encode_request(input: Self::Input) -> Result<Vec<u8>, io::Error> {
        Ok(input)
    }

    fn decode_request(request_bytes: &[u8]) -> Result<Self::Input, io::Error> {
        Ok(request_bytes.to_vec())
    }

    fn encode_response(output: Self::Output) -> Result<Vec<u8>, io::Error> {
        Ok(output)
    }

    fn decode_response(response_bytes: &[u8]) -> Result<Self::Output, io::Error> {
        Ok(response_bytes.to_vec())
    }
}
