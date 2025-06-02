use muxio::rpc::{RpcDispatcher, RpcRequest, RpcResponse, RpcStreamEvent};
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
            method_id: 0x01,
            param_bytes: Some(b"ping".to_vec()),
            pre_buffered_payload_bytes: None,
            is_finalized: true,
        };

        // Prepare a mock RPC request
        let rpc_request_2 = RpcRequest {
            method_id: 0x02,
            param_bytes: Some(b"ping2".to_vec()),
            pre_buffered_payload_bytes: None,
            is_finalized: true,
        };

        let rpc_requests = vec![rpc_request_1, rpc_request_2];

        for rpc_request in rpc_requests {
            // Move the `outgoing_buf` into the closure, ensuring it lives as long as needed
            client_dispatcher
                .call(
                    rpc_request,
                    4,
                    {
                        // Move the outgoing_buf into the closure to extend its lifetime
                        let outgoing_buf = Rc::clone(&outgoing_buf);

                        // The closure captures `outgoing_buf` and borrows it while executing
                        move |bytes: &[u8]| {
                            // Collect bytes into the buffer
                            outgoing_buf.borrow_mut().extend(bytes);

                            // Simulate echoing back the received payload
                            // The following lines would normally simulate a server echoing back the payload
                            // let reply_bytes = if bytes == b"ping" {
                            //     b"pong".to_vec() // Echo response "pong"
                            // } else {
                            //     b"fail".to_vec() // Error message if something else is received
                            // };

                            // Instead of responding to the client dispatcher, we're appending the reply to the buffer
                            //  outgoing_buf.borrow_mut().extend(reply_bytes);
                        }
                    },
                    Some(|rpc_stream_event: RpcStreamEvent| {
                        println!(
                            "Client received response from server: {:?}",
                            rpc_stream_event
                        );

                        match rpc_stream_event {
                            RpcStreamEvent::Header {
                                rpc_header_id,
                                rpc_header,
                            } => {
                                // This is a Header event, handle it here
                                println!(
                                    "Client received header: ID = {}, Header = {:?}",
                                    rpc_header_id, rpc_header
                                );
                                // Add your code for handling the header here
                            }
                            _ => {
                                // Handle other event types if necessary
                                // println!("Not a Header event");
                            }
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

                println!("Server received request header ID: {:?}", request_header_id);
                println!(
                    "\t{:?}: {:?}",
                    request_header_id,
                    server_dispatcher.delete_rpc_request(request_header_id)
                );

                // TODO: Don't hardcode this, but rather process the request intent and formulate a response
                // println!("{:?}", server_dispatcher.response_queue);
                server_dispatcher
                    .respond(
                        RpcResponse {
                            request_header_id,
                            pre_buffered_payload_bytes: Some(b"response response".to_vec()),
                            is_finalized: true,
                        },
                        4,
                        |bytes: &[u8]| {
                            // println!("Emitting: {:?}", &bytes);

                            client_dispatcher.receive_bytes(bytes).unwrap();
                        },
                    )
                    .unwrap();
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
