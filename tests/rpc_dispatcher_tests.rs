use muxio::rpc::{RpcDispatcher, RpcRequest, RpcResponse, rpc_internals::RpcStreamEvent};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn rpc_dispatcher_call_and_echo_response() {
    // Shared buffer for the outgoing response
    let outgoing_buf: Rc<RefCell<Vec<u8>>> = Rc::new(RefCell::new(Vec::new()));

    // Client dispatcher
    let mut client_dispatcher = RpcDispatcher::new();
    let mut server_dispatcher = RpcDispatcher::new();

    // TODO: Finish prototyping
    // server_dispatcher.register(
    //     "test-method",
    //     Box::new(
    //         |header: RpcHeader, params: Vec<u8>, mut emit_event: Box<dyn FnMut(RpcStreamEvent)>| {
    //             println!(
    //                 "TEST METHOD CALLED with header: {:?} and params: {:?}",
    //                 header, params
    //             );

    //             // You can call the event handler to send back an event if necessary
    //             emit_event(RpcStreamEvent::Header {
    //                 rpc_header_id: header.id,
    //                 rpc_header: header,
    //             });
    //         },
    //     ),
    // );

    {
        // Prepare a mock RPC request
        let rpc_request_1 = RpcRequest {
            method_id: RpcRequest::to_method_id("ping"),
            param_bytes: Some(b"ping".to_vec()),
            pre_buffered_payload_bytes: None,
            is_finalized: true,
        };

        // Prepare a mock RPC request
        let rpc_request_2 = RpcRequest {
            method_id: RpcRequest::to_method_id("ping2"),
            param_bytes: Some(b"ping2".to_vec()),
            pre_buffered_payload_bytes: None,
            is_finalized: true,
        };

        let rpc_requests = vec![rpc_request_1, rpc_request_2];

        for rpc_request in rpc_requests {
            let method_id = rpc_request.method_id; // u64 is Copy

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
                            } => {
                                assert_eq!(rpc_header.method_id, method_id);
                                println!(
                                    "Client received header: ID = {}, Header = {:?}",
                                    rpc_header_id, rpc_header
                                );
                            }
                            RpcStreamEvent::PayloadChunk {
                                rpc_header_id,
                                bytes,
                            } => {
                                println!(
                                    "Client received payload bytes: ID = {}, Bytes = {:?}",
                                    rpc_header_id, bytes
                                );
                            }
                            _ => {}
                        }
                    }),
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

                    // let method_id = rpc_request.method_id;
                    let ping_id = RpcRequest::to_method_id("ping");
                    let ping2_id = RpcRequest::to_method_id("ping2");

                    let rpc_response = match rpc_request.method_id {
                        id if id == ping_id => Some(RpcResponse {
                            request_header_id,
                            method_id: rpc_request.method_id,
                            pre_buffered_payload_bytes: Some(b"response response".to_vec()),
                            is_finalized: true,
                        }),
                        id if id == ping2_id => Some(RpcResponse {
                            request_header_id,
                            method_id: rpc_request.method_id,
                            pre_buffered_payload_bytes: Some(
                                b"response response a b c d e f g h i j".to_vec(),
                            ),
                            is_finalized: true,
                        }),
                        _ => None,
                    };

                    if let Some(rpc_response) = rpc_response {
                        // TODO: Don't hardcode this, but rather process the request intent and formulate a response
                        // println!("{:?}", server_dispatcher.response_queue);
                        server_dispatcher
                            .respond(rpc_response, 4, |bytes: &[u8]| {
                                // println!("Emitting: {:?}", &bytes);

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

    // println!("Call executed and response echoed successfully!");
}
