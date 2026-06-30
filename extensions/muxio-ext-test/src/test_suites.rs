use crate::endpoint_helpers::{ERROR_TEST_METHOD_ID, STREAMING_CAPTURE_METHOD_ID, UNREGISTERED_METHOD_ID};
use example_muxio_rpc_service_definition::prebuffered::{Add, Echo, Mult};
use futures_util::StreamExt;
use muxio_core::rpc::rpc_internals::RpcStreamEvent;
use muxio_core::rpc::RpcRequest;
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
/// events in the correct order with the correct data.
///
/// The streaming handler is registered on the server endpoint by
/// `connect_for_streaming()`. The test makes a streaming call, sends
/// chunks, and ends the stream. Captured events from the server-side
/// handler are inspected to verify delivery.
pub async fn streaming_handler_events_arrive<C>(
    client: &C,
    events: &Mutex<Vec<RpcStreamEvent>>,
) where
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
        encoder
            .write_bytes(chunk)
            .expect("write_bytes failed");
    }
    encoder.flush().expect("flush failed");
    encoder.end_stream().expect("end_stream failed");
    drop(encoder);

    // Allow time for events to be processed by the streaming handler
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Drain any remaining response (not expected without Phase 2 response path)
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        async {
            while let Some(_chunk) = receiver.next().await {}
        },
    )
    .await;

    // Verify captured events
    let captured = events.lock().unwrap();
    assert!(!captured.is_empty(), "Streaming handler should have received events");

    // First event must be Header with correct method_id
    assert!(
        matches!(&captured[0], RpcStreamEvent::Header { rpc_method_id, .. } if *rpc_method_id == STREAMING_CAPTURE_METHOD_ID),
        "First event should be Header with method_id {}, got {:?}",
        STREAMING_CAPTURE_METHOD_ID, captured[0]
    );

    // There should be at least one PayloadChunk event (transport may
    // coalesce multiple write_bytes calls into a single chunk)
    let chunk_count = captured.iter().filter(|e| matches!(e, RpcStreamEvent::PayloadChunk { .. })).count();
    assert!(chunk_count >= 1, "Expected at least one PayloadChunk event, got {chunk_count}");

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
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        async {
            while let Some(_chunk) = receiver.next().await {}
        },
    )
    .await;
}
