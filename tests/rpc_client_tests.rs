use muxio::rpc::{RpcClient, RpcHeader, RpcMessageType, RpcMuxSession, RpcStreamEvent};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

#[test]
fn rpc_client_stream_and_reply_roundtrip() {
    let client = Rc::new(RefCell::new(RpcClient::new(RpcMuxSession::new())));
    let server = Rc::new(RefCell::new(RpcClient::new(RpcMuxSession::new())));

    let mut server_inbox = Vec::new();
    let client_inbox = Rc::new(RefCell::new(Vec::new()));
    let client_received_payload = Rc::new(RefCell::new(Vec::new()));
    let client_received_metadata = Rc::new(RefCell::new(HashMap::new()));
    let server_received_payload = Rc::new(RefCell::new(Vec::new()));
    let pending = Rc::new(RefCell::new(VecDeque::new()));

    let call_header = RpcHeader {
        msg_type: RpcMessageType::Call,
        id: 1,
        method_id: 0xABCDABCDABCDABCD,
        metadata_bytes: b"req-meta".to_vec(),
    };

    {
        let recv_buf = server_received_payload.clone();
        let pending_reply = pending.clone();

        server
            .borrow_mut()
            .set_response_handler(move |evt| match evt {
                RpcStreamEvent::Header { rpc_header, .. } => {
                    assert_eq!(rpc_header.metadata_bytes, b"req-meta");
                }
                RpcStreamEvent::PayloadChunk { bytes, .. } => {
                    recv_buf.borrow_mut().extend(bytes);
                }
                RpcStreamEvent::End { rpc_header_id } => {
                    let reply_bytes = match recv_buf.borrow().as_slice() {
                        b"ping" => b"pong".as_ref(),
                        _ => b"fail".as_ref(),
                    };

                    let reply_header = RpcHeader {
                        msg_type: RpcMessageType::Response,
                        id: rpc_header_id,
                        method_id: 0xABCDABCDABCDABCD,
                        metadata_bytes: b"resp-meta".to_vec(),
                    };

                    pending_reply
                        .borrow_mut()
                        .push_back((reply_header, reply_bytes.to_vec()));
                }
                _ => {}
            });
    }

    let payload_clone = client_received_payload.clone();
    let metadata_clone = client_received_metadata.clone();

    let mut client_encoder = client
        .borrow_mut()
        .start_rpc_stream(
            call_header.clone(),
            4,
            |bytes| server_inbox.push(bytes.to_vec()),
            move |event| match event {
                RpcStreamEvent::Header {
                    rpc_header_id,
                    rpc_header,
                } => {
                    assert_eq!(rpc_header_id, call_header.id);
                    assert_eq!(rpc_header.msg_type, RpcMessageType::Response);
                    metadata_clone
                        .borrow_mut()
                        .insert(rpc_header_id, rpc_header.metadata_bytes);
                }
                RpcStreamEvent::PayloadChunk { bytes, .. } => {
                    payload_clone.borrow_mut().extend(bytes);
                }
                RpcStreamEvent::End { .. } => {}
                other => panic!("unexpected client event: {:?}", other),
            },
        )
        .expect("client start_rpc_stream failed");

    client_encoder.push_bytes(b"ping").unwrap();
    client_encoder.flush().unwrap();
    client_encoder.end_stream().unwrap();

    for chunk in server_inbox.iter() {
        server
            .borrow_mut()
            .receive_bytes(chunk)
            .expect("server receive_bytes failed");
    }

    // Now process pending reply **after** server handler completes
    for (reply_header, reply_bytes) in pending.borrow_mut().drain(..) {
        server
            .borrow_mut()
            .start_reply_stream(reply_header, 4, |bytes| {
                client_inbox.borrow_mut().push(bytes.to_vec());
            })
            .expect("start_reply_stream failed")
            .push_bytes(&reply_bytes)
            .unwrap();
    }

    // Feed response back to client
    for chunk in client_inbox.borrow().iter() {
        client
            .borrow_mut()
            .receive_bytes(chunk)
            .expect("client receive_bytes failed");
    }

    assert_eq!(client_received_payload.borrow().as_slice(), b"pong");
    assert_eq!(
        client_received_metadata.borrow().get(&1).unwrap(),
        &b"resp-meta".to_vec()
    );
}
