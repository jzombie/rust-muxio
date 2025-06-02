use muxio::rpc::{RpcDispatcher, RpcHeader, RpcMessageType, RpcRequest, RpcStreamEvent};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[test]
fn rpc_dispatcher_call_and_echo_response() {
    // Prepare a mock RPC request
    let rpc_request = RpcRequest {
        method_name: "ping".to_string(),
        param_bytes: b"ping".to_vec(),
        payload_bytes: None,
        is_finalized: true,
    };

    // Shared buffer for the outgoing response
    let outgoing_buf: Rc<RefCell<Vec<u8>>> = Rc::new(RefCell::new(Vec::new()));

    // Client dispatcher
    let mut client_dispatcher = RpcDispatcher::new();
    let mut server_dispatcher = RpcDispatcher::new();

    {
        // Move the `outgoing_buf` into the closure, ensuring it lives as long as needed
        client_dispatcher
            .call(
                rpc_request,
                4,
                {
                    // Move the outgoing_buf into the closure to extend its lifetime
                    let outgoing_buf = Rc::clone(&outgoing_buf);

                    // The closure captures `outgoing_buf` and borrows it while executing
                    move |bytes| {
                        println!("Server processing: {:?}", bytes);

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
                    println!("Received response: {:?}", rpc_stream_event)
                }),
            )
            .expect("Server call failed");
    }

    {
        let incoming_buf = outgoing_buf.clone();

        let request_header_ids = server_dispatcher
            .receive_bytes(incoming_buf.borrow().as_slice())
            .expect("Failed to receive bytes on server");

        println!("Request header ids: {:?}", request_header_ids);
        println!("TEST: {:?}", server_dispatcher.delete_rpc_request(1));

        // println!("{:?}", server_dispatcher.response_queue);
        server_dispatcher
            .start_reply_stream(
                RpcHeader {
                    msg_type: RpcMessageType::Response,
                    id: 1,
                    method_id: 0,
                    metadata_bytes: b"proto".to_vec(),
                },
                4,
                |bytes: &[u8]| {
                    // println!("Emitting: {:?}", &bytes);

                    client_dispatcher.receive_bytes(bytes);
                },
            )
            .unwrap();
    }

    // Now check if the response was properly echoed
    // assert_eq!(
    //     outgoing_buf.borrow().as_slice(),
    //     b"pingpong", // Expecting "ping" + "pong" as the echoed response
    //     "Expected response to be echoed"
    // );

    // println!("Call executed and response echoed successfully!");
}
