use muxio::rpc::{RpcClient, RpcHeader, RpcMessageType, RpcMuxSession, RpcStreamEvent};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

#[test]
fn rpc_client_stream_and_reply_roundtrip() {
    let mut client = RpcClient::new(RpcMuxSession::new());
    let mut server = RpcClient::new(RpcMuxSession::new());

    let mut server_inbox = Vec::new();
    let mut client_inbox = Vec::new();

    let client_received_payload = Rc::new(RefCell::new(Vec::new()));
    let client_received_metadata = Rc::new(RefCell::new(HashMap::new()));

    let call_header = Rc::new(RpcHeader {
        msg_type: RpcMessageType::Call,
        id: 1,
        method_id: 0xABCDABCDABCDABCD,
        metadata_bytes: b"req-meta".to_vec(),
    });

    // Client: initiate stream and send data
    let mut client_stream_encoder = client
        .start_rpc_stream(
            (*call_header).clone(),
            4,
            |bytes| server_inbox.push(bytes.to_vec()),
            {
                let payload = client_received_payload.clone();
                let metadata = client_received_metadata.clone();
                let hdr_id = call_header.id;

                move |event| match event {
                    RpcStreamEvent::Header {
                        rpc_header_id,
                        rpc_header,
                    } => {
                        assert_eq!(rpc_header_id, hdr_id);
                        assert_eq!(rpc_header.msg_type, RpcMessageType::Response);
                        metadata
                            .borrow_mut()
                            .insert(rpc_header_id, rpc_header.metadata_bytes);
                    }
                    RpcStreamEvent::PayloadChunk { bytes, .. } => {
                        payload.borrow_mut().extend(bytes);
                    }
                    RpcStreamEvent::End { .. } => {}
                    other => panic!("unexpected client event: {:?}", other),
                }
            },
        )
        .expect("client start_rpc_stream failed");

    client_stream_encoder.push_bytes(b"ping").unwrap();
    client_stream_encoder.flush().unwrap();
    client_stream_encoder.end_stream().unwrap();

    // Server: receive call stream
    for chunk in server_inbox.iter() {
        server
            .receive_bytes(chunk)
            .expect("server receive_bytes failed");
    }

    // Server: reply to client
    let reply_header = RpcHeader {
        msg_type: RpcMessageType::Response,
        id: call_header.id,
        method_id: call_header.method_id,
        metadata_bytes: b"resp-meta".to_vec(),
    };

    let mut server_reply_encoder = server
        .start_reply_stream(reply_header, 4, |bytes| {
            client_inbox.push(bytes.to_vec());
        })
        .expect("server start_reply_stream failed");

    server_reply_encoder.push_bytes(b"pong").unwrap();
    server_reply_encoder.flush().unwrap();
    server_reply_encoder.end_stream().unwrap();

    // Client: receive reply
    for chunk in client_inbox.iter() {
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
