use muxio::frame::{
    DecodedFrame, Frame, FrameCodec, FrameKind, FrameMuxStreamDecoder, FrameStreamEncoder,
};
use rand::seq::SliceRandom;

#[test]
fn encoder_chunks_and_decoder_recovers() {
    let mut outgoing_bytes = vec![];

    let mut encoder = FrameStreamEncoder::new(1, 5, |bytes: &[u8]| {
        outgoing_bytes.push(bytes.to_vec());
    });

    encoder
        .write_bytes(b"abcdefghijk")
        .expect("push bytes failed");
    encoder.flush().expect("flush failed");
    encoder.end_stream().expect("end stream failed");

    let mut decoder = FrameMuxStreamDecoder::new();
    let mut incoming_frames: Vec<DecodedFrame> = vec![];

    for outgoing_frame_bytes in outgoing_bytes.iter() {
        for decoded_frame_result in decoder.read_bytes(outgoing_frame_bytes) {
            if let Ok(decoded_frame) = decoded_frame_result {
                incoming_frames.push(decoded_frame);
            } else {
                panic!("decode failed")
            }
        }
    }

    assert_eq!(incoming_frames.len(), 4); // 3 data frames + 1 end frame
    assert_eq!(incoming_frames[0].inner.seq_id, 0);
    assert_eq!(incoming_frames[1].inner.seq_id, 1);
    assert_eq!(incoming_frames[2].inner.seq_id, 2);
    assert_eq!(incoming_frames[3].inner.seq_id, 3);

    let joined_incoming_frames: Vec<u8> = incoming_frames[..3]
        .iter()
        .flat_map(|f| f.inner.payload.clone())
        .collect();
    assert_eq!(joined_incoming_frames, b"abcdefghijk");
}

#[test]
fn decoder_handles_incomplete_input() {
    let frame = Frame {
        stream_id: 2,
        seq_id: 0,
        kind: FrameKind::Ping,
        timestamp_micros: 0,
        payload: b"xyz".to_vec(),
    };

    let full = FrameCodec::encode(&frame);
    let split = full.split_at(full.len() / 2);

    let mut decoder = FrameMuxStreamDecoder::new();

    {
        let decoded_frames: Vec<_> = decoder.read_bytes(split.0).collect();
        assert_eq!(decoded_frames.len(), 0); // Incomplete frame
    }

    let decoded_frames: Vec<_> = decoder.read_bytes(split.1).collect();
    assert_eq!(decoded_frames.len(), 1); // Now complete

    let out = decoded_frames[0].as_ref().expect("expected valid frame");
    assert_eq!(out.inner.payload, b"xyz");
    assert_eq!(out.inner.kind, FrameKind::Ping);
}

#[test]
fn decoder_handles_interleaved_input_order() {
    let mut stream1_outgoing_bytes = vec![];
    let mut stream2_outgoing_bytes = vec![];

    let mut encoder1 = FrameStreamEncoder::new(100, 8, |bytes: &[u8]| {
        stream1_outgoing_bytes.push(bytes.to_vec());
    });

    let mut encoder2 = FrameStreamEncoder::new(200, 8, |bytes: &[u8]| {
        stream2_outgoing_bytes.push(bytes.to_vec());
    });

    encoder1
        .write_bytes(b"stream-one-data")
        .expect("encoder1 push bytes failed");
    encoder1.flush().expect("encoder1 flush failed");
    encoder1.end_stream().expect("encoder1 end stream failed");

    encoder2
        .write_bytes(b"stream-two-data")
        .expect("encoder1 push bytes failed");
    encoder2.flush().expect("encoder2 flush failed");
    encoder2.end_stream().expect("encoder2 end stream failed");

    // Interleave emitted bytes from both streams
    let mut interleaved_outgoing_bytes = vec![];
    let mut i1 = stream1_outgoing_bytes.into_iter();
    let mut i2 = stream2_outgoing_bytes.into_iter();

    loop {
        let mut progress = false;

        if let Some(x) = i1.next() {
            interleaved_outgoing_bytes.push(x);
            progress = true;
        }

        if let Some(x) = i2.next() {
            interleaved_outgoing_bytes.push(x);
            progress = true;
        }

        if !progress {
            break;
        }
    }

    // Decode interleaved stream
    let mut decoder = FrameMuxStreamDecoder::new();
    let mut incoming_frames = vec![];

    for outgoing_bytes in interleaved_outgoing_bytes {
        for decoded_frame_result in decoder.read_bytes(&outgoing_bytes) {
            incoming_frames.push(decoded_frame_result.unwrap());
        }
    }

    // Split decoded payloads per stream
    let mut one = vec![];
    let mut two = vec![];

    for frame in incoming_frames {
        match frame.inner.stream_id {
            100 => one.extend(frame.inner.payload),
            200 => two.extend(frame.inner.payload),
            _ => panic!("Unexpected stream_id"),
        }
    }

    assert_eq!(one, b"stream-one-data");
    assert_eq!(two, b"stream-two-data");
}

#[test]
fn decoder_reorders_out_of_order_frames() {
    let mut outgoing_chunks = vec![];

    let mut encoder = FrameStreamEncoder::new(123, 3, |bytes: &[u8]| {
        outgoing_chunks.push(bytes.to_vec());
    });

    encoder
        .write_bytes(b"abcdefghi")
        .expect("push bytes failed"); // 9 bytes, should emit 3 chunks
    encoder.flush().expect("flush failed"); // emit any partials
    encoder.end_stream().expect("end stream failed"); // emit end frame

    // Shuffle to simulate out-of-order delivery
    let mut shuffled_outgoing_chunks = outgoing_chunks.clone();
    shuffled_outgoing_chunks.shuffle(&mut rand::rng());

    let mut decoder = FrameMuxStreamDecoder::new();
    let mut incoming_frames = vec![];

    for outgoing_chunk in shuffled_outgoing_chunks {
        for decoded_result in decoder.read_bytes(&outgoing_chunk) {
            incoming_frames.push(decoded_result.unwrap());
        }
    }

    // Should have 4 frames: 3 data + 1 end
    assert_eq!(incoming_frames.len(), 4);
    for (i, frame) in incoming_frames.iter().enumerate() {
        assert_eq!(frame.inner.seq_id, i as u32);
    }

    let full: Vec<u8> = incoming_frames[..3]
        .iter()
        .flat_map(|f| f.inner.payload.clone())
        .collect();
    assert_eq!(full, b"abcdefghi");
}

#[test]
fn encoder_emits_small_final_frame_on_end_stream() {
    let mut outgoing_bytes = vec![];

    let mut encoder = FrameStreamEncoder::new(999, 1_000_000, |bytes: &[u8]| {
        outgoing_bytes.extend(bytes.to_vec());
    });

    encoder.write_bytes(b"tiny").expect("push bytes failed");

    encoder.flush().expect("flush failed");
    encoder.end_stream().expect("end stream failed");

    let mut decoder = FrameMuxStreamDecoder::new();

    let mut incoming_frames = vec![];

    for decoded_frame_result in decoder.read_bytes(&outgoing_bytes) {
        incoming_frames.push(decoded_frame_result.unwrap());
    }

    assert_eq!(incoming_frames.len(), 2);

    let incoming_frame0 = &incoming_frames[0];
    assert_eq!(incoming_frame0.inner.stream_id, 999);
    assert_eq!(incoming_frame0.inner.kind, FrameKind::Open);
    assert_eq!(incoming_frame0.inner.payload, b"tiny");

    let incoming_frame1 = &incoming_frames[1];
    assert_eq!(incoming_frame1.inner.stream_id, 999);
    assert_eq!(incoming_frame1.inner.kind, FrameKind::End);
    assert_eq!(incoming_frame1.inner.payload, b"");
}

#[test]
fn encode_decode_roundtrip() {
    let original = Frame {
        stream_id: 42,
        seq_id: 0,
        kind: FrameKind::Data,
        timestamp_micros: 12345678,
        payload: b"hello world".to_vec(),
    };

    let encoded_bytes = FrameCodec::encode(&original);
    let decoded_frame = FrameCodec::decode(&encoded_bytes).expect("decode failed");

    assert_eq!(original.stream_id, decoded_frame.inner.stream_id);
    assert_eq!(original.kind, decoded_frame.inner.kind);
    assert_eq!(
        original.timestamp_micros,
        decoded_frame.inner.timestamp_micros
    );
    assert_eq!(original.payload, decoded_frame.inner.payload);
}
