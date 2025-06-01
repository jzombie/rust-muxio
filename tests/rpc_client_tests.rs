use muxio::rpc::{RpcClient, RpcHeader, RpcMessageType, RpcMuxSession, RpcStreamEvent};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[test]
fn rpc_client_stream_and_reply_roundtrip() {
    let mut client = RpcClient::new(RpcMuxSession::new());
    let mut server = RpcClient::new(RpcMuxSession::new());

    let mut server_inbox = Vec::new();
    let client_inbox = Rc::new(RefCell::new(Vec::new()));

    let client_received_payload = Rc::new(RefCell::new(Vec::new()));
    let client_received_metadata = Rc::new(RefCell::new(HashMap::new()));
    let server_received_payload = Rc::new(RefCell::new(Vec::new()));

    let call_header = RpcHeader {
        msg_type: RpcMessageType::Call,
        id: 1,
        method_id: 0xABCDABCDABCDABCD,
        metadata_bytes: b"req-meta".to_vec(),
    };

    let payload_clone = client_received_payload.clone();
    let metadata_clone = client_received_metadata.clone();

    let mut client_encoder = client
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

    // Register server handler
    {
        let recv_buf = server_received_payload.clone();
        let inbox = client_inbox.clone();

        let call_id = call_header.id;
        server.set_request_handler(call_id, move |evt| match evt {
            RpcStreamEvent::Header { rpc_header, .. } => {
                assert_eq!(rpc_header.metadata_bytes, b"req-meta");
            }
            RpcStreamEvent::PayloadChunk { bytes, .. } => {
                recv_buf.borrow_mut().extend(bytes);
            }
            RpcStreamEvent::End { .. } => {
                let reply_bytes: &[u8] = match recv_buf.borrow().as_slice() {
                    b"ping" => b"pong".as_ref(),
                    _ => b"fail".as_ref(),
                };

                let reply_header = RpcHeader {
                    msg_type: RpcMessageType::Response,
                    id: call_id,
                    method_id: 0xABCDABCDABCDABCD,
                    metadata_bytes: b"resp-meta".to_vec(),
                };

                let mut reply_encoder = RpcMuxSession::new()
                    .start_rpc_stream(reply_header, 4, |bytes| {
                        inbox.borrow_mut().push(bytes.to_vec());
                    })
                    .expect("server start_reply_stream failed");

                reply_encoder.push_bytes(reply_bytes).unwrap();
                reply_encoder.flush().unwrap();
                reply_encoder.end_stream().unwrap();
            }
            _ => {}
        });
    }

    for chunk in server_inbox.iter() {
        server
            .receive_bytes(chunk)
            .expect("server receive_bytes failed");
    }

    for chunk in client_inbox.borrow().iter() {
        client
            .receive_bytes(chunk)
            .expect("client receive_bytes failed");
    }

    assert_eq!(client_received_payload.borrow().as_slice(), b"pong");
    assert_eq!(
        client_received_metadata.borrow().get(&1).unwrap(),
        &b"resp-meta".to_vec()
    );
}
