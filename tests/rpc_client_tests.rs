use muxio::rpc::{RpcClient, RpcHeader, RpcMessageType, RpcStreamEvent};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

#[test]
fn rpc_client_response_callbacks_work() {
    let mut client = RpcClient::new();
    let outbound: Rc<RefCell<VecDeque<Vec<u8>>>> = Rc::new(RefCell::new(VecDeque::new()));
    let response_buf = Rc::new(RefCell::new(Vec::new()));
    let headers = Rc::new(RefCell::new(HashMap::new()));
    let header = Rc::new(RpcHeader {
        msg_type: RpcMessageType::Call,
        id: 99,
        method_id: 0xCAFECAFE12345678,
        metadata_bytes: b"test-meta".to_vec(),
    });

    let outbound_clone = outbound.clone();
    let header_clone = header.clone();
    let headers_clone = headers.clone();
    let response_buf_clone = response_buf.clone();

    client
        .start_rpc_stream(
            (*header_clone).clone(),
            4,
            move |bytes| outbound_clone.borrow_mut().push_back(bytes.to_vec()),
            move |evt| match evt {
                RpcStreamEvent::Header {
                    rpc_header_id,
                    rpc_header,
                } => {
                    assert_eq!(rpc_header_id, header_clone.id);
                    headers_clone
                        .borrow_mut()
                        .insert(rpc_header_id, rpc_header.metadata_bytes);
                }
                RpcStreamEvent::PayloadChunk { bytes, .. } => {
                    response_buf_clone.borrow_mut().extend(bytes);
                }
                RpcStreamEvent::End { .. } => {}
                evt => panic!("Unexpected event: {:?}", evt),
            },
        )
        .expect("failed to start RPC stream");

    let echo_header = RpcHeader {
        msg_type: RpcMessageType::Response,
        id: 99,
        method_id: header.method_id,
        metadata_bytes: b"test-meta".to_vec(),
    };

    let reply_chunks = Rc::new(RefCell::new(Vec::new()));
    let reply_chunks_clone = reply_chunks.clone();

    let mut encoder = client
        .start_reply_stream(echo_header.clone(), 5, move |bytes| {
            reply_chunks_clone.borrow_mut().push(bytes.to_vec());
        })
        .expect("start_reply_stream failed");

    encoder.push_bytes(b"reply").unwrap();
    encoder.flush().unwrap();
    encoder.end_stream().unwrap();

    for chunk in reply_chunks.borrow().iter() {
        client.receive_bytes(chunk).unwrap();
    }

    assert_eq!(response_buf.borrow().as_slice(), b"reply");
    assert_eq!(headers.borrow().get(&99).unwrap(), &b"test-meta".to_vec());
}
