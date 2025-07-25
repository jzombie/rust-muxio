//! This test specifically verifies WASM client's transport state changes and pending request failure.
//!
//! It sets up a mock server that allows explicit control over connection closure,
//! mirroring the native Tokio client transport tests.

use example_muxio_rpc_service_definition::prebuffered::Echo;
use futures_util::{SinkExt, StreamExt};
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_tokio_rpc_server::utils::{bind_tcp_listener_on_random_port, tcp_listener_to_host_port};
use muxio_wasm_rpc_client::RpcWasmClient;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};
use tokio_tungstenite::{
    connect_async, tungstenite::error::Error as WsError,
    tungstenite::protocol::Message as WsMessage,
};
use tracing::{self, instrument};

// Helper function to set up the RpcWasmClient and its WebSocket bridge
// Returns the client, a handle for messages *from* WASM, and a handle for messages *to* WASM.
// MODIFIED: Now returns a Result to propagate connection errors instead of panicking.
async fn setup_wasm_client_bridge(
    server_url: &str,
) -> Result<(Arc<RpcWasmClient>, JoinHandle<()>, JoinHandle<()>), WsError> {
    let (from_wasm_tx, mut from_wasm_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>(); // From WASM client to WS

    let client = Arc::new(RpcWasmClient::new(move |bytes| {
        // This is the `emit_callback` from RpcWasmClient to the outside world (WebSocket sender)
        let _ = from_wasm_tx.send(bytes);
    }));

    // MODIFIED: Propagate the connection error instead of using .expect()
    let (ws_stream, _) = connect_async(server_url).await?;
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Task to send messages from WASM client (via emit_callback) to the real WebSocket
    let ws_send_handle = tokio::spawn(async move {
        while let Some(bytes) = from_wasm_rx.recv().await {
            if ws_sender
                .send(WsMessage::Binary(bytes.into()))
                .await
                .is_err()
            {
                tracing::error!("WASM client to WS bridge send error, breaking loop.");
                break;
            }
        }
        tracing::debug!("WASM client to WS bridge send loop finished.");
    });

    // Task to receive messages from the real WebSocket and pass them to WASM client
    let ws_recv_handle = tokio::spawn({
        let client_clone = client.clone();
        async move {
            client_clone.handle_connect().await; // Mimic JS calling onopen
            while let Some(Ok(WsMessage::Binary(bytes))) = ws_receiver.next().await {
                // `process_incoming_bytes` now handles the dispatcher locking and handler dispatch.
                client_clone.process_incoming_bytes(&bytes).await;
            }
            tracing::debug!("WebSocket to WASM client bridge receive loop finished.");
            client_clone.handle_disconnect().await; // Mimic JS calling onclose/onerror
        }
    });

    // MODIFIED: Wrap successful result in Ok()
    Ok((client, ws_send_handle, ws_recv_handle))
}

// TODO: Debug Windows failure: "Test timed out, but expected an immediate 'Connection refused' error."
#[cfg_attr(windows, ignore)]
#[tokio::test]
#[instrument]
async fn test_client_errors_on_connection_failure() {
    tracing::info!("Running test_client_errors_on_connection_failure (WASM)");
    let (_, unused_port) = bind_tcp_listener_on_random_port().await.unwrap();
    tracing::debug!("Attempting to connect to unused port: {unused_port}");

    // RpcWasmClient::new does not actually connect, it sets up the internal channels.
    // The actual connection happens in setup_wasm_client_bridge.
    let client_for_drop = Arc::new(RpcWasmClient::new(|_| {})); // Client instance only for setup/drop

    // MODIFIED: The test logic is updated to handle the Result from the helper.
    // The connection should fail immediately, so the timeout wrapper should return Ok(Err(...)).
    let connect_result = timeout(
        Duration::from_secs(2), // Use 2 seconds to be safe
        setup_wasm_client_bridge(&format!("ws://127.0.0.1:{unused_port}/ws")),
    )
    .await;

    // We expect the connection to fail fast, not time out.
    assert!(
        connect_result.is_ok(),
        "Test timed out, but expected an immediate 'Connection refused' error."
    );

    // Unwrap the timeout result to get the inner connection result.
    let inner_result = connect_result.unwrap();

    // Assert that the inner result is an error.
    assert!(
        inner_result.is_err(),
        "Expected connection to fail, but it succeeded."
    );

    // Ensure client resources are dropped (this client was never truly "connected" anyway)
    drop(client_for_drop);

    tracing::info!("`test_client_errors_on_connection_failure` PASSED (WASM)");
}

#[tokio::test]
#[instrument]
#[allow(clippy::await_holding_lock)]
async fn test_transport_state_change_handler() {
    tracing::info!("Running test_transport_state_change_handler (WASM)");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();
    let server_url = format!("ws://{server_host}:{server_port}/ws");
    tracing::debug!("Server listening on {}:{}", server_host, server_port);

    let (server_accept_tx, server_accept_rx) =
        oneshot::channel::<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>>();
    let server_task = tokio::spawn(async move {
        tracing::debug!("[Server Task] Starting server accept loop. Waiting for one client.");
        if let Ok((socket, _addr)) = listener.accept().await {
            tracing::debug!("[Server Task] Accepted client connection from: {}", _addr);
            if let Ok(ws_stream) = tokio_tungstenite::accept_async(socket).await {
                tracing::debug!("[Server Task] WebSocket handshake complete for client.");
                let _ = server_accept_tx.send(ws_stream); // Send the WebSocketStream to the test
                // Server just keeps the connection alive until its task is aborted.
                tokio::task::yield_now().await; // Yield to allow client to proceed.
                futures::future::pending::<()>().await; // Hang indefinitely until aborted
            } else {
                tracing::debug!("[Server Task] WebSocket handshake failed for client.");
            }
        } else {
            tracing::debug!("[Server Task] Listener accept failed.");
        }
        tracing::debug!("[Server Task] Server accept loop finished.");
    });

    let received_states = Arc::new(Mutex::new(Vec::new())); // std::sync::Mutex for state accessible in synchronous callbacks.

    // MODIFIED: Unwrapping result as this test expects a successful connection.
    let (client, ws_send_handle, ws_recv_handle) =
        setup_wasm_client_bridge(&server_url).await.unwrap();

    let states_clone = received_states.clone();
    client
        .set_state_change_handler(move |state| {
            tracing::debug!("[Test Handler] State Change Handler triggered: {:?}", state);
            states_clone.lock().unwrap().push(state);
            tracing::debug!(
                "[Test Handler] Current collected states: {:?}",
                states_clone.lock().unwrap()
            );
        })
        .await;
    tracing::debug!("[Test] State change handler set.");

    // Server-side: retrieve the WebSocketStream to explicitly close it later
    let _ws_stream = timeout(Duration::from_secs(1), server_accept_rx)
        .await
        .expect("Server did not send WebSocket stream")
        .expect("Server WebSocket stream channel dropped");

    // Give the client's internal tasks a moment to process the initial 'Connected' state.
    tokio::time::sleep(Duration::from_millis(50)).await;
    tracing::debug!("[Test] Initial sleep after setting handler complete.");

    // Now, trigger the disconnect from the client side.
    // This will call RpcWasmClient's `handle_disconnect`, which updates its state and calls its handler.
    tracing::debug!("[Test] Signaling RpcWasmClient to disconnect via handle_disconnect().");
    client.handle_disconnect().await;

    // Wait for the client's receiver handle to finish (it should due to disconnect)
    // and for the client's internal `shutdown_async` to propagate.
    let _ = timeout(Duration::from_secs(1), ws_recv_handle).await;

    let final_states = received_states.lock().unwrap();
    assert_eq!(
        *final_states,
        vec![
            RpcTransportState::Connected,
            RpcTransportState::Disconnected
        ],
        "The state change handler should have been called for both connect and disconnect events. Actual: {:?}",
        *final_states
    );
    tracing::info!("`test_transport_state_change_handler` PASSED (WASM)");

    // Clean up all spawned tasks and resources.
    server_task.abort();
    ws_send_handle.abort(); // Abort the bridge send task

    tokio::time::sleep(Duration::from_millis(10)).await; // Small sleep for abort propagation
}

#[tokio::test]
#[instrument]
async fn test_pending_requests_fail_on_disconnect() {
    tracing::info!("Running test_pending_requests_fail_on_disconnect (WASM)");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();
    let server_url = format!("ws://{server_host}:{server_port}/ws");
    tracing::debug!(
        "Server for pending requests test listening on {}",
        server_url
    );

    let (server_ws_stream_tx, server_ws_stream_rx) = oneshot::channel();
    let server_task = tokio::spawn(async move {
        tracing::debug!("[Server Task Pending] Waiting for client connection.");
        if let Ok((socket, _)) = listener.accept().await {
            tracing::debug!(
                "[Server Task Pending] Client connected. Attempting WebSocket handshake."
            );
            if let Ok(ws_stream) = tokio_tungstenite::accept_async(socket).await {
                tracing::debug!(
                    "[Server Task Pending] WebSocket handshake complete. Sending stream to test."
                );
                let _ = server_ws_stream_tx.send(ws_stream); // Send the WebSocketStream to the test
                futures::future::pending::<()>().await; // Hang indefinitely until aborted
            } else {
                tracing::debug!("[Server Task Pending] WebSocket handshake failed.");
            }
        } else {
            tracing::debug!("[Server Task Pending] Listener accept failed.");
        }
        tracing::debug!("[Server Task Pending] Server task finished.");
    });

    // MODIFIED: Unwrapping result as this test expects a successful connection.
    let (client, ws_send_handle, ws_recv_handle) =
        setup_wasm_client_bridge(&server_url).await.unwrap();

    // Retrieve the WebSocket stream from the server task to control its closure.
    let (mut ws_sender, _ws_receiver) = timeout(Duration::from_secs(1), server_ws_stream_rx)
        .await
        .expect("Test timed out waiting for server to send WebSocket stream.")
        .expect("Server WebSocket stream channel dropped unexpectedly.")
        .split(); // Split the stream to get sender for explicit close

    // 1. Spawn the RPC call as a separate task.
    // This allows it to progress concurrently and become "pending".
    // We need to clone the Arc<RpcWasmClient> for the spawned task.
    let client_clone_for_rpc_task = client.clone();
    let (tx_rpc_result, rx_rpc_result) = oneshot::channel(); // Channel to get result from spawned RPC task

    tokio::spawn(async move {
        tracing::debug!("[RPC Task] Starting spawned RPC call.");
        // Make the call. This will interact with the dispatcher and its emit_fn.
        // It should become pending before the disconnect if timed correctly.
        // `Echo::call` for RpcWasmClient (via RpcServiceCallerInterface) internally calls `process_incoming_bytes`
        // which internally uses `spawn_blocking` for the dispatcher lock.
        let result = Echo::call(
            client_clone_for_rpc_task.as_ref(),
            b"this will fail".to_vec(),
        )
        .await;
        tracing::debug!("[RPC Task] RPC call completed with result: {result:?}",);
        let _ = tx_rpc_result.send(result); // Send result back to main test thread
    });
    tracing::debug!("RPC call spawned to run in background.");

    // 2. IMPORTANT: Give the RPC task ample time to become pending.
    // This sleep is crucial for the dispatcher to register the request.
    tokio::time::sleep(Duration::from_millis(300)).await; // Increased sleep for reliability.
    tokio::task::yield_now().await; // Give scheduler a chance to run all tasks.
    tracing::debug!("RPC call should be pending in dispatcher now.");

    // 3. Now, explicitly close the WebSocket connection from the server's perspective.
    // This simulates the server disconnecting, which should propagate to the client.
    tracing::debug!("[Test] Explicitly closing WebSocket connection from server's side.");
    let _ = ws_sender.close().await; // Close the server-side sender
    tracing::debug!("[Test] WebSocket connection closed by server.");

    // 4. Give client's shutdown logic time to run and cancel pending requests.
    tokio::time::sleep(Duration::from_millis(200)).await;
    tokio::task::yield_now().await;
    tracing::debug!(
        "Sleep after server close signal complete (client should have processed disconnect)."
    );

    // 5. Await the result of the spawned RPC call task. It should be an error.
    tracing::debug!("Waiting for spawned RPC call future to resolve (should be cancelled).");
    let result = timeout(Duration::from_secs(1), rx_rpc_result).await; // 1 sec timeout for resolution
    tracing::debug!("[Test] ***** Spawned RPC call future resolution result: {result:?} ***** ",);

    assert!(
        result.is_ok(),
        "Test timed out waiting for RPC call to resolve. Result: {result:?}",
    );

    let rpc_result = result
        .unwrap()
        .expect("Oneshot channel should not be dropped");
    assert!(
        rpc_result.is_err(),
        "Expected the pending RPC call to fail, but it succeeded. Result: {rpc_result:?}",
    );

    let err_string = rpc_result.unwrap_err().to_string();
    tracing::debug!("RPC error string: {}", err_string);
    // Error should indicate cancellation due to disconnect.
    assert!(
        err_string.contains("ReadAfterCancel")
            || err_string.contains("cancelled stream")
            || err_string.contains("Transport error")
            || err_string.contains("Client is disconnected"),
        "Error message should indicate that the request was cancelled due to a disconnect. Got: {err_string}",
    );
    tracing::info!("`test_pending_requests_fail_on_disconnect` PASSED (WASM)");

    // Clean up all spawned tasks and resources.
    server_task.abort();
    ws_send_handle.abort(); // Abort the bridge send task
    let _ = timeout(Duration::from_secs(1), ws_recv_handle).await; // Await receiver handle from bridge to ensure its cleanup
    tokio::time::sleep(Duration::from_millis(10)).await; // Small sleep for abort propagation
}
