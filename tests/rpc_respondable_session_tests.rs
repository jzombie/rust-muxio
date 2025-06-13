use muxio::rpc::rpc_internals::{RpcHeader, RpcMessageType, RpcRespondableSession, RpcStreamEvent};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

#[test]
fn rpc_respondable_session_stream_and_reply_roundtrip() {
    let prebuffering_options = vec![true, false];

    for is_prebuffering_response in prebuffering_options {
        let client = Arc::new(Mutex::new(RpcRespondableSession::new()));
        let server = Arc::new(Mutex::new(RpcRespondableSession::new()));

        let mut server_inbox = Vec::new();
        let client_inbox = Arc::new(Mutex::new(Vec::new()));
        let client_received_payload = Arc::new(Mutex::new(Vec::new()));
        let client_received_metadata = Arc::new(Mutex::new(HashMap::new()));
        let server_received_payload = Arc::new(Mutex::new(Vec::new()));
        let pending = Arc::new(Mutex::new(VecDeque::new()));

        let call_header = RpcHeader {
            msg_type: RpcMessageType::Call,
            id: 1,
            method_id: 0xABCDABCDABCDABCD,
            metadata_bytes: b"req-meta".to_vec(),
        };

        {
            let recv_buf = Arc::clone(&server_received_payload);
            let pending_reply = Arc::clone(&pending);
            let emit = Arc::clone(&client_inbox);

            server
                .lock()
                .unwrap()
                .set_catch_all_response_handler(move |evt| match evt {
                    RpcStreamEvent::Header { rpc_header, .. } => {
                        assert_eq!(rpc_header.metadata_bytes, b"req-meta");
                    }
                    RpcStreamEvent::PayloadChunk { bytes, .. } => {
                        recv_buf.lock().unwrap().extend(bytes);
                    }
                    RpcStreamEvent::End { rpc_request_id, .. } => {
                        let reply_bytes = match recv_buf.lock().unwrap().as_slice() {
                            b"ping" => b"pong".as_ref(),
                            _ => b"fail".as_ref(),
                        };

                        let reply_header = RpcHeader {
                            msg_type: RpcMessageType::Response,
                            id: rpc_request_id,
                            method_id: 0xABCDABCDABCDABCD,
                            metadata_bytes: b"resp-meta".to_vec(),
                        };

                        pending_reply.lock().unwrap().push_back((
                            reply_header,
                            reply_bytes.to_vec(),
                            Arc::clone(&emit),
                        ));
                    }
                    _ => {}
                });
        }

        let payload_clone = Arc::clone(&client_received_payload);
        let metadata_clone = Arc::clone(&client_received_metadata);

        let mut client_encoder = client
            .lock()
            .unwrap()
            .init_respondable_request(
                call_header.clone(),
                4,
                |bytes| server_inbox.push(bytes.to_vec()),
                Some(move |event| match event {
                    RpcStreamEvent::Header {
                        rpc_request_id,
                        rpc_header,
                        ..
                    } => {
                        assert_eq!(rpc_request_id, call_header.id);
                        assert_eq!(rpc_header.msg_type, RpcMessageType::Response);
                        metadata_clone
                            .lock()
                            .unwrap()
                            .insert(rpc_request_id, rpc_header.metadata_bytes);
                    }
                    RpcStreamEvent::PayloadChunk { bytes, .. } => {
                        payload_clone.lock().unwrap().extend(bytes);
                    }
                    RpcStreamEvent::End { .. } => {}
                    other => panic!("unexpected client event: {:?}", other),
                }),
                is_prebuffering_response,
            )
            .expect("client init_request failed");

        client_encoder.write_bytes(b"ping").unwrap();
        client_encoder.flush().unwrap();
        client_encoder.end_stream().unwrap();

        for chunk in server_inbox.iter() {
            server
                .lock()
                .unwrap()
                .read_bytes(chunk)
                .expect("server read_bytes failed");
        }

        assert_eq!(
            client.lock().unwrap().get_remaining_response_handlers(),
            1,
            "client remaining response headers incorrectly identified"
        );
        assert_eq!(
            server.lock().unwrap().get_remaining_response_handlers(),
            0,
            "server remaining response headers incorrectly identified"
        );

        for (reply_header, reply_bytes, emit) in pending.lock().unwrap().drain(..) {
            let mut server_encoder = server
                .lock()
                .unwrap()
                .start_reply_stream(reply_header, 4, {
                    let emit = Arc::clone(&emit);
                    move |bytes| {
                        emit.lock().unwrap().push(bytes.to_vec());
                    }
                })
                .expect("server init_request failed");

            server_encoder.write_bytes(&reply_bytes).unwrap();
            server_encoder.flush().unwrap();
            server_encoder.end_stream().unwrap();
        }

        for chunk in client_inbox.lock().unwrap().iter() {
            client
                .lock()
                .unwrap()
                .read_bytes(chunk)
                .expect("client read_bytes failed");
        }

        assert_eq!(client_received_payload.lock().unwrap().as_slice(), b"pong");
        assert_eq!(
            client_received_metadata.lock().unwrap().get(&1).unwrap(),
            &b"resp-meta".to_vec()
        );

        assert_eq!(
            client.lock().unwrap().get_remaining_response_handlers(),
            0,
            "client remaining response headers incorrectly identified"
        );
        assert_eq!(
            server.lock().unwrap().get_remaining_response_handlers(),
            0,
            "server remaining response headers incorrectly identified"
        );
    }
}
