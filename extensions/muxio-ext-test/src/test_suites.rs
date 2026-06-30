use crate::endpoint_helpers::{
    ERROR_TEST_METHOD_ID, MPSC_CHANNEL_METHOD_ID, STREAMING_CAPTURE_METHOD_ID,
    UNREGISTERED_METHOD_ID,
};
use example_muxio_rpc_service_definition::prebuffered::{Add, Echo, Mult};
use futures_util::StreamExt;
use muxio_core::rpc::RpcRequest;
use muxio_core::rpc::rpc_internals::RpcStreamEvent;
use muxio_mpsc_adapter::ChannelCallerExt;
use muxio_rpc_service::error::{RpcServiceError, RpcServiceErrorCode};
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service_caller::dynamic_channel::DynamicChannelType;
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_rpc_service_endpoint::RpcServiceEndpointInterface;
use std::error::Error;
use std::sync::{Arc, Mutex};
use tokio::join;

/// Run the standard success roundtrip test (Add, Mult, Echo concurrent calls).
pub async fn roundtrip_success<C: RpcServiceCallerInterface>(client: &C) {
    let (res1, res2, res3, res4, res5, res6) = join!(
        Add::call(client, vec![1.0, 2.0, 3.0]),
        Add::call(client, vec![8.0, 3.0, 7.0]),
        Mult::call(client, vec![8.0, 3.0, 7.0]),
        Mult::call(client, vec![1.5, 2.5, 8.5]),
        Echo::call(client, b"testing 1 2 3".to_vec()),
        Echo::call(client, b"testing 4 5 6".to_vec()),
    );
    assert_eq!(res1.unwrap(), 6.0);
    assert_eq!(res2.unwrap(), 18.0);
    assert_eq!(res3.unwrap(), 168.0);
    assert_eq!(res4.unwrap(), 31.875);
    assert_eq!(res5.unwrap(), b"testing 1 2 3".to_vec());
    assert_eq!(res6.unwrap(), b"testing 4 5 6".to_vec());
}

/// Run the error roundtrip test — the transport already registered ERROR_TEST_METHOD_ID
/// with a failing handler. Just call it and assert error propagation.
pub async fn roundtrip_error<C: RpcServiceCallerInterface>(client: &C) {
    let request = RpcRequest {
        rpc_method_id: ERROR_TEST_METHOD_ID,
        rpc_param_bytes: Some(b"hello".to_vec()),
        rpc_prebuffered_payload_bytes: None,
        is_finalized: true,
    };
    let result = client
        .call_rpc_buffered(request, |bytes: &[u8]| bytes.to_vec())
        .await
        .unwrap();
    let (_encoder, inner) = result;
    let err = inner.unwrap_err();
    match err {
        RpcServiceError::Rpc(payload) => {
            assert!(
                payload.message.contains("test error"),
                "Expected 'test error' in '{}'",
                payload.message
            );
        }
        other => panic!("Expected RpcServiceError::Rpc, got {:?}", other),
    }
}

/// Run the large payload roundtrip test.
pub async fn roundtrip_large_payload<C: RpcServiceCallerInterface>(client: &C) {
    let large_payload = vec![42u8; 100_000];
    let result = Echo::call(client, large_payload.clone()).await;
    assert_eq!(result.unwrap(), large_payload);
}

/// Run the method-not-found test.
pub async fn roundtrip_method_not_found<C: RpcServiceCallerInterface>(client: &C) {
    let request = RpcRequest {
        rpc_method_id: UNREGISTERED_METHOD_ID,
        rpc_param_bytes: Some(b"hello".to_vec()),
        rpc_prebuffered_payload_bytes: None,
        is_finalized: true,
    };
    let result = client
        .call_rpc_buffered(request, |bytes: &[u8]| bytes.to_vec())
        .await
        .unwrap();
    let (_encoder, inner) = result;
    let err = inner.unwrap_err();
    match err {
        RpcServiceError::Rpc(payload) => {
            assert_eq!(
                payload.code,
                RpcServiceErrorCode::NotFound,
                "Expected NotFound, got {:?}",
                payload.code
            );
        }
        other => panic!("Expected RpcServiceError::Rpc, got {:?}", other),
    }
}

// ------------------------------------------------------------------
// Transport state test bodies
// ------------------------------------------------------------------

/// Assert that connecting to nothing yields a ConnectionRefused error.
pub async fn transport_connection_failure<F, Fut>(connect: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), std::io::Error>>,
{
    let result = connect().await;
    assert!(
        result.is_err(),
        "Expected connection failure, but connection succeeded"
    );
}

/// Assert the state change handler fires Connected then Disconnected.
pub async fn transport_state_change_handler<C, F, Fut>(connect: F)
where
    C: RpcServiceCallerInterface + 'static,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Arc<C>, std::io::Error>>,
{
    let client = connect().await.unwrap();
    let received_states = Arc::new(std::sync::Mutex::new(Vec::new()));
    let notify_disconnect = Arc::new(tokio::sync::Notify::new());

    let states = received_states.clone();
    let notify = notify_disconnect.clone();
    client
        .set_state_change_handler(move |state| {
            if state == RpcTransportState::Disconnected {
                notify.notify_one();
            }
            states.lock().unwrap().push(state);
        })
        .await;

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    drop(client);

    let notified = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        notify_disconnect.notified(),
    )
    .await;
    assert!(notified.is_ok(), "Timed out waiting for disconnect");

    let final_states = received_states.lock().unwrap();
    assert_eq!(
        *final_states,
        vec![
            RpcTransportState::Connected,
            RpcTransportState::Disconnected
        ],
        "Expected [Connected, Disconnected], got {:?}",
        *final_states
    );
}

// ------------------------------------------------------------------
// Server-to-client test body
// ------------------------------------------------------------------

/// Run a server-to-client Echo roundtrip.
/// The test registers an Echo handler on the client endpoint, then the
/// server calls Echo via the `ConnectionContextHandle` / `IpcConnectionContextHandle`
/// and asserts the response matches.
pub async fn server_to_client_echo<H, C, E>(
    _client: &C,
    client_endpoint: &impl RpcServiceEndpointInterface<E>,
    ctx_handle: &H,
    label: &str,
) where
    H: RpcServiceCallerInterface,
    C: RpcServiceCallerInterface,
    E: Send + Sync + Clone + 'static,
{
    client_endpoint
        .register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
            let request = Echo::decode_request(&request_bytes)?;
            Echo::encode_response(request).map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
        })
        .await
        .unwrap();

    let test_message = format!("hello from server to {} client", label).into_bytes();
    let result = Echo::call(ctx_handle, test_message.clone()).await;
    assert!(
        result.is_ok(),
        "Server-to-{label}-client Echo failed: {:?}",
        result.err()
    );
    assert_eq!(
        result.unwrap(),
        test_message,
        "{label} client echo mismatch"
    );
}

// ------------------------------------------------------------------
/// Run a server-to-client streaming Echo roundtrip.
/// The server sends data in multiple chunks, the client reassembles them
/// and echoes the complete payload back.
pub async fn server_to_client_streaming_echo<H, C, E>(
    _client: &C,
    client_endpoint: &impl RpcServiceEndpointInterface<E>,
    ctx_handle: &H,
    label: &str,
) where
    H: RpcServiceCallerInterface,
    C: RpcServiceCallerInterface,
    E: Send + Sync + Clone + 'static,
{
    client_endpoint
        .register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
            let request = Echo::decode_request(&request_bytes)?;
            Echo::encode_response(request).map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
        })
        .await
        .unwrap();

    let payload = format!("hello from server to {label} client via stream").into_bytes();

    let request = RpcRequest {
        rpc_method_id: Echo::METHOD_ID,
        rpc_param_bytes: None,
        rpc_prebuffered_payload_bytes: None,
        is_finalized: false,
    };
    let (mut encoder, mut receiver) = ctx_handle
        .call_rpc_streaming(request, DynamicChannelType::Unbounded)
        .await
        .expect("server-to-client streaming call failed");

    for chunk in payload.chunks(16) {
        encoder.write_bytes(chunk).expect("write_bytes failed");
    }
    encoder.flush().expect("flush failed");
    encoder.end_stream().expect("end_stream failed");

    let mut response = Vec::new();
    while let Some(chunk) = receiver.next().await {
        match chunk {
            Ok(bytes) => response.extend_from_slice(&bytes),
            Err(e) => panic!("{label} streaming response error: {e:?}"),
        }
    }

    assert_eq!(response, payload, "{label} streaming echo mismatch");
}

/// Run concurrent bidirectional streaming — server pushes to client and
/// client pushes to server simultaneously.  Each side sends a payload
/// in chunks, reads the echoed response, and verifies integrity.
/// This proves the transport does not deadlock when streams flow in
/// both directions at the same time.
pub async fn concurrent_bidirectional_streaming<H, C, E>(
    client: Arc<C>,
    client_endpoint: &impl RpcServiceEndpointInterface<E>,
    ctx_handle: H,
    label: &str,
) where
    H: RpcServiceCallerInterface + Clone + Send + Sync + 'static,
    C: RpcServiceCallerInterface + Send + Sync + 'static,
    E: Send + Sync + Clone + 'static,
{
    // Register Echo on the client endpoint (for server-initiated calls).
    // The server endpoint already has Echo registered by connect_s2c().
    client_endpoint
        .register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
            let request = Echo::decode_request(&request_bytes)?;
            Echo::encode_response(request).map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
        })
        .await
        .unwrap();

    // ~5 MB per direction — forces genuine multi-buffer streaming through
    // the framing layer, transport I/O, and reassembly on the receiving side.
    let server_payload = vec![b'A'; 5 * 1024 * 1024];
    let client_payload = vec![b'B'; 5 * 1024 * 1024];

    // Spawn both directions concurrently
    let handle = ctx_handle;
    let sp = server_payload.clone();
    let server_task = tokio::spawn(async move {
        let request = RpcRequest {
            rpc_method_id: Echo::METHOD_ID,
            rpc_param_bytes: None,
            rpc_prebuffered_payload_bytes: None,
            is_finalized: false,
        };
        let (mut encoder, mut receiver) = handle
            .call_rpc_streaming(request, DynamicChannelType::Unbounded)
            .await
            .expect("server-side streaming call failed");

        // Yield every few chunks so the opposite task can interleave
        // its writes — proving true concurrent bidirectional streaming.
        for (i, chunk) in sp.chunks(512).enumerate() {
            encoder
                .write_bytes(chunk)
                .expect("server write_bytes failed");
            if i % 8 == 0 {
                tokio::task::yield_now().await;
            }
        }
        encoder.flush().expect("server flush failed");
        encoder.end_stream().expect("server end_stream failed");

        let mut response = Vec::new();
        while let Some(chunk) = receiver.next().await {
            match chunk {
                Ok(bytes) => response.extend_from_slice(&bytes),
                Err(e) => panic!("server streaming response error: {e:?}"),
            }
        }
        response
    });

    let cp = client_payload.clone();
    let client_task = tokio::spawn(async move {
        let request = RpcRequest {
            rpc_method_id: Echo::METHOD_ID,
            rpc_param_bytes: None,
            rpc_prebuffered_payload_bytes: None,
            is_finalized: false,
        };
        let (mut encoder, mut receiver) = client
            .call_rpc_streaming(request, DynamicChannelType::Unbounded)
            .await
            .expect("client-side streaming call failed");

        for (i, chunk) in cp.chunks(512).enumerate() {
            encoder
                .write_bytes(chunk)
                .expect("client write_bytes failed");
            if i % 8 == 0 {
                tokio::task::yield_now().await;
            }
        }
        encoder.flush().expect("client flush failed");
        encoder.end_stream().expect("client end_stream failed");

        let mut response = Vec::new();
        while let Some(chunk) = receiver.next().await {
            match chunk {
                Ok(bytes) => response.extend_from_slice(&bytes),
                Err(e) => panic!("client streaming response error: {e:?}"),
            }
        }
        response
    });

    let (server_result, client_result) = tokio::join!(server_task, client_task);
    assert_eq!(
        server_result.expect("server task panicked"),
        server_payload,
        "{label} server→client data mismatch"
    );
    assert_eq!(
        client_result.expect("client task panicked"),
        client_payload,
        "{label} client→server data mismatch"
    );
}

/// Send a large number of tiny messages through a single stream and
/// report throughput — simulating a stream of mouse or keyboard events.
/// The server opens a stream, writes N small payloads (one per "event"),
/// finalizes, and reads the echo.
pub async fn stream_small_messages_throughput<H, C, E>(
    _client: &C,
    client_endpoint: &impl RpcServiceEndpointInterface<E>,
    ctx_handle: H,
    label: &str,
) where
    H: RpcServiceCallerInterface + Send + Sync + 'static,
    C: RpcServiceCallerInterface + Send + Sync + 'static,
    E: Send + Sync + Clone + 'static,
{
    client_endpoint
        .register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
            let request = Echo::decode_request(&request_bytes)?;
            Echo::encode_response(request).map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
        })
        .await
        .unwrap();

    // Simulate 10,000 mouse events at 8 bytes each
    let num_events = 10_000u32;
    let event_size = 8usize;
    let total_bytes = num_events as usize * event_size;

    let request = RpcRequest {
        rpc_method_id: Echo::METHOD_ID,
        rpc_param_bytes: None,
        rpc_prebuffered_payload_bytes: None,
        is_finalized: false,
    };
    let (mut encoder, mut receiver) = ctx_handle
        .call_rpc_streaming(request, DynamicChannelType::Unbounded)
        .await
        .expect("streaming call failed");

    let start = std::time::Instant::now();
    let mut event_buf = vec![0u8; event_size];
    for i in 0..num_events {
        // Pack an 8-byte "mouse event"
        event_buf[0..4].copy_from_slice(&i.to_le_bytes());
        // event_buf[4..8] reserved for flags
        encoder.write_bytes(&event_buf).expect("write_bytes failed");
        if i % 100 == 0 {
            tokio::task::yield_now().await;
        }
    }
    encoder.flush().expect("flush failed");
    encoder.end_stream().expect("end_stream failed");

    let mut response = Vec::new();
    while let Some(chunk) = receiver.next().await {
        match chunk {
            Ok(bytes) => response.extend_from_slice(&bytes),
            Err(e) => panic!("response error: {e:?}"),
        }
    }

    let elapsed = start.elapsed();
    let events_per_sec = num_events as f64 / elapsed.as_secs_f64();
    let mbps = (total_bytes as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64();

    assert_eq!(
        response.len(),
        total_bytes,
        "{label} throughput data size mismatch: expected {total_bytes}, got {}",
        response.len()
    );

    // Rate-limited to avoid CI noise; just verify every event ID
    for i in 0..num_events {
        let offset = i as usize * event_size;
        let id = u32::from_le_bytes(response[offset..offset + 4].try_into().unwrap());
        assert_eq!(id, i, "{label} event {i} out of order at offset {offset}");
    }

    println!(
        "[{label}] {num_events} × {event_size}B events via stream: \
         {:.0} events/s, {:.2} MB/s, {elapsed:.2?}",
        events_per_sec, mbps,
    );
}

// ------------------------------------------------------------------
// Pending requests fail on disconnect
// ------------------------------------------------------------------

/// Assert that pending RPC calls fail when the transport disconnects.
pub async fn pending_requests_fail_on_disconnect<C, D, Dfut>(client: Arc<C>, trigger_disconnect: D)
where
    C: RpcServiceCallerInterface + 'static,
    D: FnOnce() -> Dfut,
    Dfut: std::future::Future<Output = ()>,
{
    let client_clone = client.clone();
    let (tx_result, rx_result) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = Echo::call(client_clone.as_ref(), b"will fail".to_vec()).await;
        let _ = tx_result.send(result);
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    trigger_disconnect().await;

    let result = tokio::time::timeout(std::time::Duration::from_secs(3), rx_result)
        .await
        .expect("Timed out waiting for RPC call to resolve")
        .expect("Oneshot channel dropped");

    assert!(
        result.is_err(),
        "Expected the pending RPC call to fail, but it succeeded."
    );
}

// ------------------------------------------------------------------
// Streaming handler test bodies
// ------------------------------------------------------------------

/// Verify that a streaming handler receives Header, PayloadChunk, End
/// events in the correct order and can send response chunks back
/// via the StreamResponder.
pub async fn streaming_handler_events_arrive<C>(client: &C, events: &Mutex<Vec<RpcStreamEvent>>)
where
    C: RpcServiceCallerInterface,
{
    let payload = b"hello streaming world".to_vec();

    let request = RpcRequest {
        rpc_method_id: STREAMING_CAPTURE_METHOD_ID,
        rpc_param_bytes: None,
        rpc_prebuffered_payload_bytes: None,
        is_finalized: false,
    };

    let (mut encoder, mut receiver) = client
        .call_rpc_streaming(request, DynamicChannelType::Unbounded)
        .await
        .expect("streaming call failed");

    // Write data in chunks
    for chunk in payload.chunks(8) {
        encoder.write_bytes(chunk).expect("write_bytes failed");
    }
    encoder.flush().expect("flush failed");
    encoder.end_stream().expect("end_stream failed");
    drop(encoder);

    // Allow time for events to be processed by the streaming handler
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Verify captured events
    {
        let captured = events.lock().unwrap();
        assert!(
            !captured.is_empty(),
            "Streaming handler should have received events"
        );

        // First event must be Header with correct method_id
        assert!(
            matches!(&captured[0], RpcStreamEvent::Header { rpc_method_id, .. } if *rpc_method_id == STREAMING_CAPTURE_METHOD_ID),
            "First event should be Header with method_id {}, got {:?}",
            STREAMING_CAPTURE_METHOD_ID,
            captured[0]
        );

        // There should be at least one PayloadChunk event (transport may
        // coalesce multiple write_bytes calls into a single chunk)
        let chunk_count = captured
            .iter()
            .filter(|e| matches!(e, RpcStreamEvent::PayloadChunk { .. }))
            .count();
        assert!(
            chunk_count >= 1,
            "Expected at least one PayloadChunk event, got {chunk_count}"
        );

        // Last event must be End
        assert!(
            matches!(captured.last(), Some(RpcStreamEvent::End { .. })),
            "Last event should be End, got {:?}",
            captured.last()
        );

        // Verify total payload data matches
        let total_received: usize = captured
            .iter()
            .filter_map(|e| {
                if let RpcStreamEvent::PayloadChunk { bytes, .. } = e {
                    Some(bytes.len())
                } else {
                    None
                }
            })
            .sum();
        assert_eq!(total_received, payload.len(), "Total payload data mismatch");
    }

    let mut response = Vec::new();
    if let Ok(resp) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(chunk) = receiver.next().await {
            match chunk {
                Ok(bytes) => response.extend_from_slice(&bytes),
                Err(_) => break,
            }
        }
        std::mem::take(&mut response)
    })
    .await
        && !resp.is_empty()
    {
        assert_eq!(
            resp, payload,
            "Streaming handler echoed back mismatched payload"
        );
    }
}

/// Verify that a streaming call to an unregistered method does not hang.
/// The server may or may not produce a response for unregistered streaming
/// methods — the test just verifies the call doesn't deadlock.
pub async fn streaming_handler_method_not_found<C>(client: &C)
where
    C: RpcServiceCallerInterface,
{
    let request = RpcRequest {
        rpc_method_id: UNREGISTERED_METHOD_ID,
        rpc_param_bytes: Some(b"hello".to_vec()),
        rpc_prebuffered_payload_bytes: None,
        is_finalized: false,
    };

    let (mut encoder, mut receiver) = client
        .call_rpc_streaming(request, DynamicChannelType::Unbounded)
        .await
        .expect("streaming call should succeed initially");

    encoder.end_stream().expect("end_stream failed");
    drop(encoder);

    // Drain receiver with timeout — ensures no deadlock even if
    // the server doesn't send a response
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(_chunk) = receiver.next().await {}
    })
    .await;
}

/// Run multiple concurrent streams from both parties interleaved with
/// prebuffered RPC calls on the same connection.
///
/// Exercises:
/// - 3 concurrent client→server streaming calls
/// - 3 concurrent server→client streaming calls (simultaneous bidirectional)
/// - Prebuffered (unary) RPCs interleaved between stream chunks
/// - True concurrent bidirectional streaming at the transport level
pub async fn complex_concurrent_mixed<H, C, E>(
    client: Arc<C>,
    client_endpoint: &impl RpcServiceEndpointInterface<E>,
    ctx_handle: H,
    label: &str,
) where
    H: RpcServiceCallerInterface + Clone + Send + Sync + 'static,
    C: RpcServiceCallerInterface + Send + Sync + 'static,
    E: Send + Sync + Clone + 'static,
{
    client_endpoint
        .register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
            Echo::encode_response(Echo::decode_request(&request_bytes)?)
                .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
        })
        .await
        .unwrap();

    let client_stream_tasks: Vec<_> = (0..3)
        .map(|i| {
            let client = client.clone();
            tokio::spawn(async move {
                let request = RpcRequest {
                    rpc_method_id: Echo::METHOD_ID,
                    rpc_param_bytes: None,
                    rpc_prebuffered_payload_bytes: None,
                    is_finalized: false,
                };
                let (mut encoder, mut receiver) = client
                    .call_rpc_streaming(request, DynamicChannelType::Unbounded)
                    .await
                    .expect("client stream failed to start");

                let payload = vec![b'A' + i as u8; 100_000];
                for chunk in payload.chunks(1024) {
                    encoder.write_bytes(chunk).unwrap();
                    if i % 2 == 0 {
                        tokio::task::yield_now().await;
                    }
                }
                encoder.flush().unwrap();
                encoder.end_stream().unwrap();

                let mut response = Vec::new();
                while let Some(chunk) = receiver.next().await {
                    match chunk {
                        Ok(bytes) => response.extend_from_slice(&bytes),
                        Err(e) => panic!("client stream {i} error: {e:?}"),
                    }
                }
                (i, response)
            })
        })
        .collect();

    let server_stream_tasks: Vec<_> = (0..3)
        .map(|i| {
            let handle = ctx_handle.clone();
            tokio::spawn(async move {
                let request = RpcRequest {
                    rpc_method_id: Echo::METHOD_ID,
                    rpc_param_bytes: None,
                    rpc_prebuffered_payload_bytes: None,
                    is_finalized: false,
                };
                let (mut encoder, mut receiver) = handle
                    .call_rpc_streaming(request, DynamicChannelType::Unbounded)
                    .await
                    .expect("server stream failed to start");

                let payload = vec![b'X' + i as u8; 100_000];
                for chunk in payload.chunks(1024) {
                    encoder.write_bytes(chunk).unwrap();
                    if i % 2 == 0 {
                        tokio::task::yield_now().await;
                    }
                }
                encoder.flush().unwrap();
                encoder.end_stream().unwrap();

                let mut response = Vec::new();
                while let Some(chunk) = receiver.next().await {
                    match chunk {
                        Ok(bytes) => response.extend_from_slice(&bytes),
                        Err(e) => panic!("server stream {i} error: {e:?}"),
                    }
                }
                (i, response)
            })
        })
        .collect();

    let mut prebuffered_results = Vec::new();
    for i in 0..5 {
        let sum = Add::call(&*client, vec![i as f64, (i + 1) as f64])
            .await
            .unwrap();
        prebuffered_results.push(sum);
        tokio::task::yield_now().await;
    }

    for task in client_stream_tasks {
        let (i, response) = task.await.unwrap();
        let expected = vec![b'A' + i as u8; 100_000];
        assert_eq!(
            response, expected,
            "{label} client stream {i} data mismatch"
        );
    }

    for task in server_stream_tasks {
        let (i, response) = task.await.unwrap();
        let expected = vec![b'X' + i as u8; 100_000];
        assert_eq!(
            response, expected,
            "{label} server stream {i} data mismatch"
        );
    }

    assert_eq!(
        prebuffered_results,
        vec![1.0, 3.0, 5.0, 7.0, 9.0],
        "{label} prebuffered results mismatch"
    );
}

// ------------------------------------------------------------------
// mpsc adapter test bodies
// ------------------------------------------------------------------

/// Verify that the client can `open_channel` and roundtrip data through a
/// streaming handler on the server end.
///
/// The server already has a streaming echo handler for `STREAMING_CAPTURE_METHOD_ID`
/// (set up by `connect_for_streaming`).
pub async fn mpsc_adapter_open_channel<C>(client: &C)
where
    C: RpcServiceCallerInterface,
{
    let payload = b"hello mpsc channel".to_vec();

    let (writer, mut reader) = client
        .open_channel(STREAMING_CAPTURE_METHOD_ID, 0)
        .await
        .expect("open_channel failed");

    // Write the payload in chunks via the sender
    for chunk in payload.chunks(8) {
        writer.send(chunk.to_vec()).unwrap();
    }
    // Drop the writer to signal end-of-stream to the background task
    drop(writer);

    // Collect the echoed response
    let mut response = Vec::new();
    while let Some(chunk) = reader.recv().await {
        match chunk {
            Ok(bytes) => response.extend_from_slice(&bytes),
            Err(_) => break,
        }
    }

    assert_eq!(
        response, payload,
        "Channel adapter echoed back mismatched payload"
    );
}

/// Verify that opening a channel to an unregistered method does not hang.
pub async fn mpsc_adapter_open_channel_method_not_found<C>(client: &C)
where
    C: RpcServiceCallerInterface,
{
    let (writer, mut reader) = client
        .open_channel(UNREGISTERED_METHOD_ID, 0)
        .await
        .expect("open_channel should succeed initially");

    writer.send(b"hello".to_vec()).unwrap();
    drop(writer);

    // Drain with timeout — ensures no deadlock
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(_chunk) = reader.recv().await {}
    })
    .await;
}

/// Verify `register_channel_handler` works for server-to-client streaming.
///
/// Registers a channel handler on the client endpoint, then has the server
/// initiate a streaming call to that method. The handler's receiver gets the data.
pub async fn mpsc_adapter_channel_handler_s2c<C, H>(
    _client: Arc<C>,
    client_endpoint: &impl muxio_rpc_service_endpoint::RpcServiceEndpointInterface<()>,
    server_handle: H,
) where
    C: RpcServiceCallerInterface + Send + Sync + 'static,
    H: RpcServiceCallerInterface + Clone + Send + Sync + 'static,
{
    use muxio_mpsc_adapter::ChannelEndpointExt;
    use tokio::sync::mpsc;

    let (tx, mut rx) = mpsc::unbounded_channel();
    client_endpoint
        .register_channel_handler(MPSC_CHANNEL_METHOD_ID, tx)
        .await
        .expect("Failed to register channel handler");

    let request = RpcRequest {
        rpc_method_id: MPSC_CHANNEL_METHOD_ID,
        rpc_param_bytes: None,
        rpc_prebuffered_payload_bytes: None,
        is_finalized: false,
    };

    let payload = b"hello from server".to_vec();
    let (mut encoder, _receiver) = server_handle
        .call_rpc_streaming(request, DynamicChannelType::Unbounded)
        .await
        .expect("server streaming call failed");

    encoder.write_bytes(&payload).expect("write_bytes failed");
    encoder.flush().expect("flush failed");
    encoder.end_stream().expect("end_stream failed");
    drop(encoder);

    // Read from the channel handler's receiver
    let mut received = Vec::new();
    while let Some(chunk) = rx.recv().await {
        received.extend_from_slice(&chunk);
    }

    assert_eq!(
        received, payload,
        "Channel handler received mismatched payload"
    );
}

#[cfg(test)]
mod method_id_uniqueness_tests {
    use example_muxio_rpc_service_definition::RpcMethodPrebuffered;
    use std::collections::HashSet;

    #[test]
    fn all_known_method_ids_are_unique() {
        let mut seen = HashSet::new();
        let test_ids = vec![
            crate::endpoint_helpers::STREAMING_CAPTURE_METHOD_ID,
            crate::endpoint_helpers::ERROR_TEST_METHOD_ID,
            crate::endpoint_helpers::UNREGISTERED_METHOD_ID,
            example_muxio_rpc_service_definition::prebuffered::Add::METHOD_ID,
            example_muxio_rpc_service_definition::prebuffered::Mult::METHOD_ID,
            example_muxio_rpc_service_definition::prebuffered::Echo::METHOD_ID,
        ];
        for id in test_ids {
            assert!(
                seen.insert(id),
                "Duplicate method ID detected: {}. All method IDs must be unique.",
                id
            );
        }
    }
}
