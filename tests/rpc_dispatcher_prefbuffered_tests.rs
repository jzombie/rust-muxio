use bitcode::{Decode, Encode};
use muxio::rpc::{RpcDispatcher, RpcRequest, RpcResponse, rpc_internals::RpcStreamEvent};

#[test]
fn test_rpc_dispatcher_prebuffered_calls() {
    // In a real-world application, only one of these dispatchers would exist
    // locally depending on whether you're implementing the client or server.
    // Here
    let mut client_dispatcher: RpcDispatcher<'_> = RpcDispatcher::new();
    let mut server_dispatcher: RpcDispatcher<'_> = RpcDispatcher::new();

    {
        let inputs = vec![1.0, 2.0, 3.0];
        let expected: f64 = inputs.iter().sum();
        let result = add(
            &mut client_dispatcher,
            &mut server_dispatcher,
            inputs.clone(),
        );
        assert!(
            (result - expected).abs() < 1e-10,
            "Expected {expected}, got {result}",
        );
    }

    {
        let inputs = vec![4.0, 5.0, 6.0, std::f64::consts::PI];
        let expected: f64 = inputs.iter().product();
        let result = mult(
            &mut client_dispatcher,
            &mut server_dispatcher,
            inputs.clone(),
        );
        assert!(
            (result - expected).abs() < 1e-10,
            "Expected {expected}, got {result}",
        );
    }

    {
        let inputs = vec![10.0, 5.0, 6.0, std::f64::consts::PI];
        let expected: f64 = inputs.iter().product();
        let result = mult(
            &mut client_dispatcher,
            &mut server_dispatcher,
            inputs.clone(),
        );
        assert!(
            (result - expected).abs() < 1e-10,
            "Expected {expected}, got {result}",
        );
    }
}

const ADD_METHOD_ID: u64 = 0x01;
const MULT_METHOD_ID: u64 = 0x02;

fn add(
    client_dispatcher: &mut RpcDispatcher,
    server_dispatcher: &mut RpcDispatcher,
    numbers: Vec<f64>,
) -> f64 {
    let decoded: AddResponseParams = dispatch_call_and_get_prebuffered_response(
        client_dispatcher,
        server_dispatcher,
        ADD_METHOD_ID,
        bitcode::encode(&AddRequestParams { numbers }),
    );

    decoded.result
}

fn mult(
    client_dispatcher: &mut RpcDispatcher,
    server_dispatcher: &mut RpcDispatcher,
    numbers: Vec<f64>,
) -> f64 {
    let decoded: AddResponseParams = dispatch_call_and_get_prebuffered_response(
        client_dispatcher,
        server_dispatcher,
        MULT_METHOD_ID,
        bitcode::encode(&MultRequestParams { numbers }),
    );

    decoded.result
}

#[derive(Encode, Decode, PartialEq, Debug)]
struct AddRequestParams {
    numbers: Vec<f64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
struct AddResponseParams {
    result: f64,
}

#[derive(Encode, Decode, PartialEq, Debug)]
struct MultRequestParams {
    numbers: Vec<f64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
struct MultResponseParams {
    result: f64,
}

fn encode_request(rpc_method_id: u64, rpc_param_bytes: Vec<u8>) -> RpcRequest {
    RpcRequest {
        rpc_method_id,
        rpc_param_bytes: Some(rpc_param_bytes),
        rpc_prebuffered_payload_bytes: None,
        is_finalized: true,
    }
}

/// Dispatches a call and returns the decoded response.
///
/// In a real network setup, the client and server would reside on different
/// machines or contexts and communicate over sockets. However, for the purposes
/// of this test, both the client and server dispatchers are instantiated and
/// used locally to simulate full end-to-end behavior.
fn dispatch_call_and_get_prebuffered_response<T: for<'a> Decode<'a>>(
    client_dispatcher: &mut RpcDispatcher,
    server_dispatcher: &mut RpcDispatcher,
    method_id: u64,
    rpc_param_bytes: Vec<u8>,
) -> T {
    let mut outgoing_buf = Vec::new();
    let result_buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    let rpc_request = encode_request(method_id, rpc_param_bytes);
    let result_buf_clone = result_buf.clone();

    client_dispatcher
        .call(
            rpc_request,
            4,
            |bytes: &[u8]| {
                outgoing_buf.extend_from_slice(bytes);
            },
            Some(move |rpc_stream_event: RpcStreamEvent| {
                if let RpcStreamEvent::PayloadChunk { bytes, .. } = rpc_stream_event {
                    result_buf_clone.lock().unwrap().extend_from_slice(&bytes);
                }
            }),
            true,
        )
        .expect("Server call failed");

    for chunk in outgoing_buf.chunks(4) {
        let request_ids = server_dispatcher
            .read_bytes(chunk)
            .expect("Failed to receive bytes on server");

        for rpc_request_id in request_ids {
            if !server_dispatcher
                .is_rpc_request_finalized(rpc_request_id)
                .unwrap()
            {
                continue;
            }

            let rpc_request = server_dispatcher.delete_rpc_request(rpc_request_id);

            if let Some(rpc_request) = rpc_request {
                let rpc_response = match rpc_request.rpc_method_id {
                    rpc_method_id if rpc_method_id == ADD_METHOD_ID => {
                        let request_params: AddRequestParams =
                            bitcode::decode(&rpc_request.rpc_param_bytes.unwrap()).unwrap();

                        Some(RpcResponse {
                            rpc_request_id,
                            rpc_method_id,
                            rpc_result_status: Some(0),
                            rpc_prebuffered_payload_bytes: Some(bitcode::encode(
                                &AddResponseParams {
                                    result: request_params.numbers.iter().sum(),
                                },
                            )),
                            is_finalized: true,
                        })
                    }
                    rpc_method_id if rpc_method_id == MULT_METHOD_ID => {
                        let request_params: MultRequestParams =
                            bitcode::decode(&rpc_request.rpc_param_bytes.unwrap()).unwrap();

                        Some(RpcResponse {
                            rpc_request_id,
                            rpc_method_id,
                            rpc_result_status: Some(0),
                            rpc_prebuffered_payload_bytes: Some(bitcode::encode(
                                &MultResponseParams {
                                    result: request_params.numbers.iter().product(),
                                },
                            )),
                            is_finalized: true,
                        })
                    }
                    _ => unimplemented!("Unhandled request route"),
                };

                if let Some(rpc_response) = rpc_response {
                    server_dispatcher
                        .respond(rpc_response, 4, |bytes: &[u8]| {
                            // Write the response back to the client dispatcher
                            client_dispatcher.read_bytes(bytes).unwrap();
                        })
                        .unwrap();
                }
            }
        }
    }

    let result_buf_locked = result_buf.lock().unwrap();
    bitcode::decode(&result_buf_locked).unwrap()
}
