use muxio::rpc::{RpcClient, RpcHeader, RpcMessageType, RpcMuxSession, RpcStreamEvent};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

#[test]
fn rpc_client_response_callbacks_work() {
    let client_mux = RpcMuxSession::new();
    let mut client_rpc = RpcClient::new(client_mux);
    let mut server_mux = RpcMuxSession::new();

    let client_outbox = Rc::new(RefCell::new(VecDeque::<Vec<u8>>::new()));
    let client_received_payload = Rc::new(RefCell::new(Vec::new()));
    let client_received_metadata = Rc::new(RefCell::new(HashMap::new()));

    let call_header = Rc::new(RpcHeader {
        msg_type: RpcMessageType::Call,
        id: 99,
        method_id: 0xCAFECAFE12345678,
        metadata_bytes: b"test-meta-request".to_vec(),
    });

    {
        let tx_buf = client_outbox.clone();
        let hdr = call_header.clone();
        let metadata = client_received_metadata.clone();
        let payload = client_received_payload.clone();

        client_rpc
            .start_rpc_stream(
                (*hdr).clone(),
                4,
                move |bytes| tx_buf.borrow_mut().push_back(bytes.to_vec()),
                move |evt| match evt {
                    RpcStreamEvent::Header {
                        rpc_header_id,
                        rpc_header,
                    } => {
                        assert_eq!(rpc_header.msg_type, RpcMessageType::Response);

                        assert_eq!(rpc_header_id, hdr.id);
                        metadata
                            .borrow_mut()
                            .insert(rpc_header_id, rpc_header.metadata_bytes);
                    }
                    RpcStreamEvent::PayloadChunk { bytes, .. } => {
                        payload.borrow_mut().extend(bytes);
                    }
                    RpcStreamEvent::End { .. } => {}
                    other => panic!("Unexpected event: {:?}", other),
                },
            )
            .expect("failed to start RPC stream");
    }

    // Pipe client->server
    while let Some(chunk) = client_outbox.borrow_mut().pop_front() {
        server_mux
            .receive_bytes(&chunk, |evt| {
                // server-side event sink — not needed in this test
                match evt {
                    RpcStreamEvent::Header {
                        rpc_header_id,
                        rpc_header,
                    } => {
                        assert_eq!(rpc_header.msg_type, RpcMessageType::Call);
                        assert_eq!(rpc_header_id, call_header.id);
                        assert_eq!(rpc_header.metadata_bytes, b"test-meta-request")
                    }
                    _ => {}
                }
            })
            .unwrap();
    }

    let response_header = RpcHeader {
        msg_type: RpcMessageType::Response,
        id: 99,
        method_id: call_header.method_id,
        metadata_bytes: b"test-meta-response".to_vec(),
    };

    let server_outbox = Rc::new(RefCell::new(Vec::new()));
    let out = server_outbox.clone();

    let mut response_stream = server_mux
        .start_rpc_stream(response_header, 5, move |bytes| {
            out.borrow_mut().push(bytes.to_vec());
        })
        .expect("server reply stream failed");

    response_stream.push_bytes(b"reply").unwrap();
    response_stream.flush().unwrap();
    response_stream.end_stream().unwrap();

    // Pipe server->client
    for chunk in server_outbox.borrow().iter() {
        client_rpc.receive_bytes(chunk).unwrap();
    }

    assert_eq!(client_received_payload.borrow().as_slice(), b"reply");
    assert_eq!(
        client_received_metadata.borrow().get(&99).unwrap(),
        &b"test-meta-response".to_vec()
    );
}
