use bitcode::{Decode, Encode};
use muxio::rpc::rpc_internals::{RpcHeader, RpcMessageType, RpcSession, RpcStreamEvent};
use rand::prelude::SliceRandom;
use std::cell::RefCell;
use std::collections::HashMap;

#[test]
fn rpc_parallel_streams_roundtrip() {
    let mut client = RpcSession::new();
    let mut server = RpcSession::new();

    // The final validation logic for payload correctness and metadata integrity
    let mut decoded: HashMap<u32, (Option<RpcHeader>, Vec<u8>)> = HashMap::new();

    // RefCell for processing received bytes on a simulated server
    let process_received_bytes = RefCell::new(|bytes: &[u8]| {
        server
            .read_bytes(bytes, |evt| match evt {
                RpcStreamEvent::Header {
                    rpc_header_id,
                    ref rpc_header,
                    ..
                } => {
                    // Validate headers and store the header with its msg ID
                    match rpc_header.id {
                        // 1 => assert_eq!(rpc_header.metadata["x"], "1".into()),
                        // 2 => assert_eq!(rpc_header.metadata["y"], "2".into()),
                        1 => assert_eq!(rpc_header.metadata_bytes, b"message 1 metadata"),
                        2 => assert_eq!(rpc_header.metadata_bytes, b"message 2 metadata"),
                        _ => panic!("unexpected header ID"),
                    }

                    assert!(
                        rpc_header_id == rpc_header.id,
                        "rpc_header_id should be rpc_header.id"
                    );

                    decoded.entry(rpc_header_id).or_default().0 = Some(rpc_header.clone());
                }
                RpcStreamEvent::PayloadChunk {
                    rpc_header_id,
                    bytes,
                    ..
                } => {
                    decoded.entry(rpc_header_id).or_default().1.extend(bytes);
                }
                RpcStreamEvent::End { rpc_header_id, .. } => {
                    assert!(decoded.contains_key(&rpc_header_id))
                }
                _ => {}
            })
            .unwrap();
    });

    // Setup headers for both streams
    let hdr1 = RpcHeader {
        msg_type: RpcMessageType::Call,
        id: 1,
        method_id: 0xaaaabbbbccccdddd,
        // metadata: [("x".into(), "1".into())].into(),
        metadata_bytes: b"message 1 metadata".into(),
    };
    let hdr2 = RpcHeader {
        msg_type: RpcMessageType::Call,
        id: 2,
        method_id: 0x1111222233334444,
        // metadata: [("y".into(), "2".into())].into(),
        metadata_bytes: b"message 2 metadata".into(),
    };

    // Prepare the payloads for both streams
    let mut payload1 = b"hello on stream 100".chunks(8).map(Vec::from);
    let mut payload2 = b"hello on stream 200".chunks(8).map(Vec::from);

    // Start the first stream (stream 1)
    let mut enc1 = client
        .init_request(hdr1, 8, |bytes: &[u8]| {
            // Borrow the `RefCell` mutably and call the closure
            process_received_bytes.borrow_mut()(bytes);
        })
        .expect("enc1 could not be instantiated");

    // Start the second stream (stream 2)
    let mut enc2 = client
        .init_request(hdr2, 4, |bytes: &[u8]| {
            // Borrow the `RefCell` mutably and call the closure
            process_received_bytes.borrow_mut()(bytes);
        })
        .expect("enc2 could not be instantiated");

    // Ensure both streams have different IDs
    assert!(
        enc1.stream_id() != enc2.stream_id(),
        "stream_id1 and stream_id2 should be different"
    );

    // Write to encoders in an interleaved fashion
    loop {
        let mut progressed = false;

        if let Some(chunk) = payload1.next() {
            enc1.write_bytes(&chunk).expect("enc1 push bytes failed");
            progressed = true;
        }

        if let Some(chunk) = payload2.next() {
            enc2.write_bytes(&chunk).expect("enc2 push bytes failed");
            progressed = true;
        }

        if !progressed {
            break;
        }
    }

    // Flush and end both streams
    enc1.flush().expect("enc1 flush failed");
    enc1.end_stream().expect("enc1 end stream failed");

    enc2.flush().expect("enc2 flush failed");
    enc2.end_stream().expect("enc2 end stream failed");

    // Validate `metadata`` integrity
    assert_eq!(
        decoded.get(&1).unwrap().0.as_ref().unwrap().metadata_bytes,
        b"message 1 metadata"
    );
    assert_eq!(
        decoded.get(&2).unwrap().0.as_ref().unwrap().metadata_bytes,
        b"message 2 metadata"
    );

    // Validate `method_id` integrity
    assert_eq!(
        decoded.get(&1).unwrap().0.as_ref().unwrap().method_id,
        0xaaaabbbbccccdddd
    );
    assert_eq!(
        decoded.get(&2).unwrap().0.as_ref().unwrap().method_id,
        0x1111222233334444
    );

    // Validate `payload` integrity
    assert_eq!(decoded.get(&1).unwrap().1, b"hello on stream 100");
    assert_eq!(decoded.get(&2).unwrap().1, b"hello on stream 200");
}

#[test]
fn rpc_stream_with_multiple_metadata_entries() {
    let mut client = RpcSession::new();
    let mut server = RpcSession::new();

    let mut outbound: Vec<u8> = Vec::new();

    #[derive(Encode, Decode, PartialEq, Debug)]
    struct Metadata {
        foo: String,
        baz: String,
        alpha: String,
        gamma: String,
        truthy: bool,
        falsy: bool,
        some_integer: u8,
        some_float: f32,
        some_bool_vec: Vec<bool>,
    }

    let metadata_bytes = bitcode::encode(&Metadata {
        foo: "bar".into(),
        baz: "qux".into(),
        alpha: "beta".into(),
        gamma: "delta".into(),
        truthy: true,
        falsy: false,
        some_integer: 42,
        some_float: 3.14,
        some_bool_vec: vec![true, false, true],
    });

    // Create a header with multiple metadata entries
    let hdr = RpcHeader {
        msg_type: RpcMessageType::Call,
        id: 1,
        method_id: 0x1234,
        metadata_bytes,
    };

    // Start RPC stream with the header containing multiple metadata entries
    let mut enc = client
        .init_request(hdr, 10, |bytes: &[u8]| {
            outbound.extend(bytes.to_vec());
        })
        .expect("enc instantiation failed");

    enc.write_bytes(b"test multiple metadata")
        .expect("push bytes failed");

    enc.flush().expect("enc flush failed");
    enc.end_stream().expect("enc end stream failed");

    // Process the frames on the server side
    let mut decoded: HashMap<u32, (Option<RpcHeader>, Vec<u8>)> = HashMap::new();
    for chunk in outbound.chunks(8).map(Vec::from) {
        server
            .read_bytes(&chunk, |evt| match evt {
                RpcStreamEvent::Header {
                    rpc_header_id,
                    ref rpc_header,
                    ..
                } => {
                    decoded.entry(rpc_header_id).or_default().0 = Some(rpc_header.clone());
                }
                RpcStreamEvent::PayloadChunk {
                    rpc_header_id,
                    bytes,
                    ..
                } => {
                    decoded.entry(rpc_header_id).or_default().1.extend(bytes);
                }
                RpcStreamEvent::End { .. } => {}
                _ => {}
            })
            .unwrap();
    }

    let decoded_metadata =
        bitcode::decode::<Metadata>(&decoded.get(&1).unwrap().0.as_ref().unwrap().metadata_bytes)
            .expect("metadata deserilization failed");

    // Verify payload correctness and metadata integrity
    assert_eq!(decoded.get(&1).unwrap().1, b"test multiple metadata");

    assert_eq!(decoded_metadata.foo, "bar".to_string());
    assert_eq!(decoded_metadata.baz, "qux".to_string());
    assert_eq!(decoded_metadata.alpha, "beta".to_string());
    assert_eq!(decoded_metadata.gamma, "delta".to_string());
    assert_eq!(decoded_metadata.truthy, true);
    assert_eq!(decoded_metadata.falsy, false);
    assert_eq!(decoded_metadata.some_integer, 42);
    assert_eq!(decoded_metadata.some_float, 3.14);
    assert_eq!(decoded_metadata.some_bool_vec, vec![true, false, true]);
}

#[test]
fn rpc_complex_shuffled_stream() {
    let mut client = RpcSession::new();
    let mut server = RpcSession::new();

    // Grouped by chunks so that they can be shuffled
    let outbound_chunks: RefCell<Vec<Vec<u8>>> = RefCell::new(vec![]);

    #[derive(Encode, Decode, PartialEq, Debug)]
    struct Metadata {
        foo: String,
        baz: String,
        alpha: String,
        gamma: String,
        truthy: bool,
        falsy: bool,
        some_integer: u8,
        some_float: f32,
        some_bool_vec: Vec<bool>,
    }

    let metadata_bytes_1 = bitcode::encode(&Metadata {
        foo: "bar".into(),
        baz: "qux".into(),
        alpha: "beta".into(),
        gamma: "delta".into(),
        truthy: true,
        falsy: false,
        some_integer: 42,
        some_float: 3.14,
        some_bool_vec: vec![true, false, true],
    });

    // Create a header with multiple metadata entries
    let hdr_1 = RpcHeader {
        msg_type: RpcMessageType::Call,
        id: 1,
        method_id: 0x1234,
        metadata_bytes: metadata_bytes_1,
    };

    let metadata_bytes_2 = bitcode::encode(&Metadata {
        foo: "bar2".into(),
        baz: "qux2".into(),
        alpha: "beta2".into(),
        gamma: "delta2".into(),
        truthy: false,
        falsy: true,
        some_integer: 142,
        some_float: 14.3,
        some_bool_vec: vec![false, true, false],
    });

    let hdr_2 = RpcHeader {
        msg_type: RpcMessageType::Event,
        id: 2,
        method_id: 0x5678,
        metadata_bytes: metadata_bytes_2,
    };

    // Start RPC stream with the header containing multiple metadata entries
    let mut enc1 = client
        .init_request(hdr_1, 10, |bytes: &[u8]| {
            outbound_chunks.borrow_mut().push(bytes.to_vec())
        })
        .expect("enc1 instantiation failed");

    let mut enc2 = client
        .init_request(hdr_2, 10, |bytes: &[u8]| {
            outbound_chunks.borrow_mut().push(bytes.to_vec())
        })
        .expect("enc2 instantiation failed");

    enc1.write_bytes(b"Shuffled payload bytes 1 2 3 4 5")
        .expect("enc1 write_bytes failed");

    enc2.write_bytes(b"Shuffled payload bytes 6 7 8 9 10")
        .expect("enc1 write_bytes failed");

    enc1.flush().expect("enc1 flush failed");
    enc1.end_stream().expect("enc1 end stream failed");

    enc2.flush().expect("enc2 flush failed");
    enc2.end_stream().expect("enc2 end stream failed");

    // Run this sequence multiple times with newly shuffled entries
    for _ in [0..10] {
        // Randomize the outbound frames ordder
        outbound_chunks.borrow_mut().shuffle(&mut rand::rng());

        // Process the frames on the server side
        let mut decoded: HashMap<u32, (Option<RpcHeader>, Vec<u8>)> = HashMap::new();
        for chunk in outbound_chunks.borrow().iter() {
            server
                .read_bytes(&chunk, |evt| match evt {
                    RpcStreamEvent::Header {
                        rpc_header_id,
                        ref rpc_header,
                        ..
                    } => {
                        decoded.entry(rpc_header_id).or_default().0 = Some(rpc_header.clone());
                    }
                    RpcStreamEvent::PayloadChunk {
                        rpc_header_id,
                        bytes,
                        ..
                    } => {
                        decoded.entry(rpc_header_id).or_default().1.extend(bytes);
                    }
                    RpcStreamEvent::End { .. } => {}
                    _ => {}
                })
                .unwrap();
        }

        let decoded_metadata_1 = bitcode::decode::<Metadata>(
            &decoded.get(&1).unwrap().0.as_ref().unwrap().metadata_bytes,
        )
        .expect("metadata_1 deserilization failed");
        let decoded_metadata_2 = bitcode::decode::<Metadata>(
            &decoded.get(&2).unwrap().0.as_ref().unwrap().metadata_bytes,
        )
        .expect("metadata_2 deserilization failed");

        // Verify payload correctness and metadata integrity
        assert_eq!(decoded.get(&1).unwrap().0.as_ref().unwrap().id, 1);
        assert_eq!(decoded.get(&2).unwrap().0.as_ref().unwrap().id, 2);

        assert_eq!(
            decoded.get(&1).unwrap().0.as_ref().unwrap().msg_type,
            RpcMessageType::Call
        );
        assert_eq!(
            decoded.get(&2).unwrap().0.as_ref().unwrap().msg_type,
            RpcMessageType::Event
        );

        assert_eq!(
            decoded.get(&1).unwrap().0.as_ref().unwrap().method_id,
            0x1234
        );
        assert_eq!(
            decoded.get(&2).unwrap().0.as_ref().unwrap().method_id,
            0x5678
        );

        assert_eq!(decoded_metadata_1.foo, "bar".to_string());
        assert_eq!(decoded_metadata_2.foo, "bar2".to_string());

        assert_eq!(decoded_metadata_1.baz, "qux".to_string());
        assert_eq!(decoded_metadata_2.baz, "qux2".to_string());

        assert_eq!(decoded_metadata_1.alpha, "beta".to_string());
        assert_eq!(decoded_metadata_2.alpha, "beta2".to_string());

        assert_eq!(decoded_metadata_1.gamma, "delta".to_string());
        assert_eq!(decoded_metadata_2.gamma, "delta2".to_string());

        assert_eq!(decoded_metadata_1.truthy, true);
        assert_eq!(decoded_metadata_2.truthy, false);

        assert_eq!(decoded_metadata_1.falsy, false);
        assert_eq!(decoded_metadata_2.falsy, true);

        assert_eq!(decoded_metadata_1.some_integer, 42);
        assert_eq!(decoded_metadata_2.some_integer, 142);

        assert_eq!(decoded_metadata_1.some_float, 3.14);
        assert_eq!(decoded_metadata_2.some_float, 14.3);

        assert_eq!(decoded_metadata_1.some_bool_vec, vec![true, false, true]);
        assert_eq!(decoded_metadata_2.some_bool_vec, vec![false, true, false]);

        assert_eq!(
            decoded.get(&1).unwrap().1,
            b"Shuffled payload bytes 1 2 3 4 5"
        );
        assert_eq!(
            decoded.get(&2).unwrap().1,
            b"Shuffled payload bytes 6 7 8 9 10"
        );
    }
}

#[test]
fn rpc_session_bidirectional_roundtrip() {
    let mut client = RpcSession::new();
    let mut server = RpcSession::new();

    let hdr = RpcHeader {
        msg_type: RpcMessageType::Call,
        id: 42,
        method_id: 0x123,
        metadata_bytes: b"foo-bar".to_vec(),
    };

    let mut outbound = Vec::new();

    let mut enc = client
        .init_request(hdr.clone(), 2, |bytes: &[u8]| {
            outbound.push(bytes.to_vec());
        })
        .expect("init_request failed");

    for chunk in b"ping".chunks(2) {
        enc.write_bytes(chunk).expect("push_chunk failed");
    }

    enc.flush().expect("flush failed");
    enc.end_stream().expect("end_stream failed");

    let mut req_buf = Vec::new();
    let mut seen_hdr = None;

    for chunk in &outbound {
        server
            .read_bytes(chunk, |evt| match evt {
                RpcStreamEvent::Header {
                    rpc_header_id: _,
                    rpc_header,
                    ..
                } => {
                    assert_eq!(rpc_header.metadata_bytes, b"foo-bar");
                    seen_hdr = Some(rpc_header);
                }
                RpcStreamEvent::PayloadChunk {
                    rpc_header_id: _,
                    bytes,
                    ..
                } => {
                    req_buf.extend(bytes);
                }
                RpcStreamEvent::End { .. } => {}
                _ => {}
            })
            .expect("server.read_bytes failed");
    }

    assert_eq!(req_buf, b"ping");
    assert!(seen_hdr.is_some());

    // Send a reply back
    let reply_hdr = RpcHeader {
        msg_type: RpcMessageType::Response,
        id: hdr.id,
        method_id: hdr.method_id,
        metadata_bytes: b"baz-qux".to_vec(),
    };

    let mut reply_bytes = Vec::new();

    let mut reply_enc = server
        .init_request(reply_hdr.clone(), 2, |bytes: &[u8]| {
            reply_bytes.push(bytes.to_vec());
        })
        .expect("init_request (reply) failed");

    for chunk in b"pong".chunks(2) {
        reply_enc.write_bytes(chunk).expect("push_chunk (reply)");
    }

    reply_enc.flush().expect("flush (reply)");
    reply_enc.end_stream().expect("end_stream (reply)");

    let mut reply_buf = Vec::new();
    let mut reply_hdr_seen = None;

    for chunk in &reply_bytes {
        client
            .read_bytes(chunk, |evt| match evt {
                RpcStreamEvent::Header {
                    rpc_header_id: _,
                    rpc_header,
                    ..
                } => {
                    assert_eq!(rpc_header.metadata_bytes, b"baz-qux");
                    reply_hdr_seen = Some(rpc_header);
                }
                RpcStreamEvent::PayloadChunk {
                    rpc_header_id: _,
                    bytes,
                    ..
                } => {
                    reply_buf.extend(bytes);
                }
                RpcStreamEvent::End { .. } => {}
                _ => {}
            })
            .expect("client.read_bytes failed");
    }

    assert_eq!(reply_buf, b"pong");
    assert!(reply_hdr_seen.is_some());
}
