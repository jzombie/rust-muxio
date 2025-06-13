use muxio::{
    frame::{FrameDecodeError, FrameEncodeError, FrameMuxStreamDecoder, FrameStreamEncoder},
    rpc::rpc_internals::{RpcHeader, RpcMessageType, RpcSession},
};
use std::cell::RefCell;

#[test]
fn cancel_stream_does_not_process_after_cancellation() {
    let mut outgoing_bytes: Vec<u8> = vec![];

    let mut encoder = FrameStreamEncoder::new(42, 10, |bytes: &[u8]| {
        outgoing_bytes.extend(bytes);
    });

    // Data to encode and decode
    let data = b"some regular data";

    encoder
        .write_bytes(data)
        .expect("encoder push bytes failed");

    // Now simulate the canceling of the stream
    let result = encoder.cancel_stream();
    assert!(
        !result.is_err(),
        "Original `cancel_stream` attempt should not error"
    );

    // Subsequent attempt to cancel fails
    let result = encoder.cancel_stream();
    assert!(matches!(result, Err(FrameEncodeError::WriteAfterCancel)));

    // Attempt to end fails (already terminated)
    let result: Result<usize, FrameEncodeError> = encoder.end_stream();
    assert!(matches!(result, Err(FrameEncodeError::WriteAfterCancel)));

    // Now that the stream is canceled, we should not process any further frames.
    // Try to push more data after canceling the stream
    let result = encoder.write_bytes(b"more data after cancel");
    assert!(matches!(result, Err(FrameEncodeError::WriteAfterCancel)));

    let mut decoder = FrameMuxStreamDecoder::new();

    let mut has_stream_termination = false;

    for decode_result in decoder.read_bytes(&outgoing_bytes) {
        if matches!(
            decode_result.unwrap().decode_error,
            Some(FrameDecodeError::ReadAfterCancel)
        ) {
            has_stream_termination = true;
            break;
        }
    }

    assert_eq!(has_stream_termination, true);
}

#[test]
fn rpc_stream_aborts_on_cancel_frame() {
    let mut client = RpcSession::new();
    let mut server = RpcSession::new();

    let hdr = RpcHeader {
        rpc_msg_type: RpcMessageType::Call,
        rpc_request_id: 42,
        rpc_method_id: 0x1234,
        rpc_metadata_bytes: b"test metadata bytes".into(),
    };

    let decoder_error: RefCell<Option<FrameDecodeError>> = RefCell::new(None);

    // RefCell for processing received bytes on a simulated server
    let process_received_bytes =
        RefCell::new(
            |bytes: &[u8]| match server.read_bytes(bytes, |_evt| Ok(())) {
                Ok(()) => {}
                Err(err) => *decoder_error.borrow_mut() = Some(err),
            },
        );

    // Start a new RPC stream
    let mut enc = client
        .init_request(hdr, 10, |bytes: &[u8]| {
            process_received_bytes.borrow_mut()(bytes);
        })
        .expect("enc instantiation failed");

    enc.write_bytes(b"testing 1 2 3")
        .expect("enc push bytes failed");
    enc.write_bytes(b"testing 4 5 6")
        .expect("enc push bytes failed");
    enc.write_bytes(b"testing 7 8 9")
        .expect("enc push bytes failed");
    assert!(decoder_error.borrow().is_none());

    // Send the cancel frame immediately to abort the stream
    enc.cancel_stream().expect("enc cancel stream failed");

    assert!(matches!(
        *decoder_error.borrow(),
        Some(FrameDecodeError::ReadAfterCancel)
    ));

    enc.write_bytes(b"testing 7 8 9")
        .expect_err("enc should no longer allow push bytes");
}

#[test]
fn rpc_stream_aborts_on_end_frame() {
    let mut client = RpcSession::new();
    let mut server = RpcSession::new();

    let hdr = RpcHeader {
        rpc_msg_type: RpcMessageType::Call,
        rpc_request_id: 42,
        rpc_method_id: 0x1234,
        rpc_metadata_bytes: b"test metadata bytes".into(),
    };

    let decoder_error: RefCell<Option<FrameDecodeError>> = RefCell::new(None);

    // RefCell for processing received bytes on a simulated server
    let process_received_bytes =
        RefCell::new(
            |bytes: &[u8]| match server.read_bytes(bytes, |_evt| Ok(())) {
                Ok(()) => {}
                Err(err) => *decoder_error.borrow_mut() = Some(err),
            },
        );

    // Start a new RPC stream
    let mut enc = client
        .init_request(hdr, 10, |bytes: &[u8]| {
            process_received_bytes.borrow_mut()(bytes);
        })
        .expect("enc instantiation failed");

    enc.write_bytes(b"testing 1 2 3")
        .expect("enc push bytes failed");
    enc.write_bytes(b"testing 4 5 6")
        .expect("enc push bytes failed");
    enc.write_bytes(b"testing 7 8 9")
        .expect("enc push bytes failed");
    assert!(decoder_error.borrow().is_none());

    // Send the cancel frame immediately to abort the stream
    enc.end_stream().expect("enc end stream failed");

    enc.write_bytes(b"testing 7 8 9")
        .expect_err("enc should no longer allow push bytes");

    enc.cancel_stream()
        .expect_err("enc should no longer allow stream cancel");
}

#[test]
fn end_stream_auto_flushes_buffer() {
    let mut outgoing_bytes: Vec<u8> = vec![];

    let mut encoder = FrameStreamEncoder::new(42, 10, |bytes: &[u8]| {
        outgoing_bytes.extend_from_slice(bytes);
    });

    let mut decoder = FrameMuxStreamDecoder::new();

    // Data to encode and decode
    let data = b"some regular data";

    // Push data into encoder, will emit chunks <= 10 bytes each
    encoder.write_bytes(data).expect("encode data");

    // Finalize the stream
    encoder.end_stream().expect("end stream");

    let push_past_result = encoder.write_bytes(b"additional data that should not be present");
    assert!(matches!(
        push_past_result,
        Err(FrameEncodeError::WriteAfterEnd)
    ));

    let mut decoded_frames = vec![];

    for result in decoder.read_bytes(&outgoing_bytes) {
        let frame = result.expect("frame decoding failed");
        decoded_frames.push(frame);
    }

    // Aggregate the payloads of all frames (should be reassembled in order)
    let decoded_payload: Vec<u8> = decoded_frames
        .into_iter()
        .filter(|f| f.inner.payload.len() > 0) // exclude cancel/end marker-only frames
        .flat_map(|f| f.inner.payload)
        .collect();

    assert_eq!(
        decoded_payload, data,
        "Decoded data should match the original input"
    );
}
