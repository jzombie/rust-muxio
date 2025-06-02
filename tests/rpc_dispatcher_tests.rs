use bitcode::{Decode, Encode};
use muxio::rpc::{RpcDispatcher, RpcRequest, RpcResponse, rpc_internals::RpcStreamEvent};
use std::cell::RefCell;
use std::rc::Rc;

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

#[test]
fn rpc_dispatcher_call_and_echo_response() {
    // Shared buffer for the outgoing response
    let outgoing_buf: Rc<RefCell<Vec<u8>>> = Rc::new(RefCell::new(Vec::new()));

    // Client dispatcher
    let mut client_dispatcher = RpcDispatcher::new();
    let mut server_dispatcher = RpcDispatcher::new();

    {
        // Prepare a mock RPC request
        let rpc_request_1 = RpcRequest {
            method_id: RpcRequest::to_method_id("add"),
            param_bytes: Some(bitcode::encode(&AddRequestParams {
                numbers: vec![1.0, 2.0, 3.0],
            })),
            pre_buffered_payload_bytes: None,
            is_finalized: true,
        };

        // Prepare a mock RPC request
        let rpc_request_2 = RpcRequest {
            method_id: RpcRequest::to_method_id("mult"),
            param_bytes: Some(bitcode::encode(&MultRequestParams {
                numbers: vec![4.0, 5.0, 6.0, 3.14],
            })),
            pre_buffered_payload_bytes: None,
            is_finalized: true,
        };

        let rpc_requests = vec![rpc_request_1, rpc_request_2];

        for rpc_request in rpc_requests {
            let method_id = rpc_request.method_id;

            // Create a new buffer for each request
            // let buffer = Rc::new(RefCell::new(Vec::new()));

            // Store the buffer in the map with its associated request header ID
            // request_buffers.insert(rpc_request.header_id, Rc::clone(&buffer));

            client_dispatcher
                .call(
                    rpc_request,
                    4,
                    {
                        let outgoing_buf = Rc::clone(&outgoing_buf);
                        move |bytes: &[u8]| {
                            outgoing_buf.borrow_mut().extend(bytes);
                        }
                    },
                    Some({
                        move |rpc_stream_event: RpcStreamEvent| match rpc_stream_event {
                            RpcStreamEvent::Header {
                                rpc_header_id,
                                rpc_header,
                                rpc_method_id,
                            } => {
                                assert_eq!(rpc_header.method_id, method_id);
                                assert_eq!(rpc_method_id, method_id);
                                println!(
                                    "Client received header: ID = {}, Header = {:?}",
                                    rpc_header_id, rpc_header
                                );
                            }
                            RpcStreamEvent::PayloadChunk {
                                bytes,
                                rpc_method_id,
                                ..
                            } => match rpc_method_id {
                                id if id == RpcRequest::to_method_id("add") => {
                                    println!(
                                        "Add response: {:?}",
                                        bitcode::decode::<AddResponseParams>(&bytes)
                                    );
                                }
                                id if id == RpcRequest::to_method_id("mult") => {
                                    println!(
                                        "Mult response: {:?}",
                                        bitcode::decode::<MultResponseParams>(&bytes)
                                    );
                                }
                                _ => {
                                    unreachable!("Unhandled response");
                                }
                            },
                            _ => {}
                        }
                    }),
                    true,
                )
                .expect("Server call failed");
        }
    }

    {
        let incoming_buf = outgoing_buf.clone();
        let chunk_size = 4; // Define the chunk size
        for chunk in incoming_buf.borrow().chunks(chunk_size) {
            let request_header_ids = server_dispatcher
                .receive_bytes(chunk)
                .expect("Failed to receive bytes on server");

            for request_header_id in request_header_ids {
                let is_request_finalized = server_dispatcher
                    .is_rpc_request_finalized(request_header_id)
                    .unwrap();

                // Pre-buffer entire request
                if !is_request_finalized {
                    continue;
                }

                let rpc_request = server_dispatcher.delete_rpc_request(request_header_id);

                if let Some(rpc_request) = rpc_request {
                    println!("Server received request header ID: {:?}", request_header_id);
                    println!("\t{:?}: {:?}", request_header_id, rpc_request);

                    let rpc_response = match rpc_request.method_id {
                        id if id == RpcRequest::to_method_id("add") => {
                            let request_params: AddRequestParams =
                                bitcode::decode(&rpc_request.param_bytes.unwrap()).unwrap();

                            println!("Server received request params: {:?}", request_params);

                            let response_bytes = bitcode::encode(&AddResponseParams {
                                result: request_params.numbers.iter().sum(),
                            });

                            Some(RpcResponse {
                                request_header_id,
                                method_id: rpc_request.method_id,
                                pre_buffered_payload_bytes: Some(response_bytes),
                                is_finalized: true,
                            })
                        }

                        id if id == RpcRequest::to_method_id("mult") => {
                            let request_params: MultRequestParams =
                                bitcode::decode(&rpc_request.param_bytes.unwrap()).unwrap();

                            println!("Server received request params: {:?}", request_params);

                            let response_bytes = bitcode::encode(&MultResponseParams {
                                result: request_params.numbers.iter().fold(1.0, |acc, &x| acc * x),
                            });

                            Some(RpcResponse {
                                request_header_id,
                                method_id: rpc_request.method_id,
                                pre_buffered_payload_bytes: Some(response_bytes),
                                is_finalized: true,
                            })
                        }
                        _ => None,
                    };

                    if let Some(rpc_response) = rpc_response {
                        // TODO: Don't hardcode this, but rather process the request intent and formulate a response
                        server_dispatcher
                            .respond(rpc_response, 4, |bytes: &[u8]| {
                                client_dispatcher.receive_bytes(bytes).unwrap();
                            })
                            .unwrap();
                    }
                }
            }
        }
    }

    // Now check if the response was properly echoed
    // assert_eq!(
    //     outgoing_buf.borrow().as_slice(),
    //     b"pingpong", // Expecting "ping" + "pong" as the echoed response
    //     "Expected response to be echoed"
    // );
}
