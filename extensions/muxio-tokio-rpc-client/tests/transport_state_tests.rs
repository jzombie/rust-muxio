use example_muxio_rpc_service_definition::prebuffered::Echo;
use futures_util::{SinkExt, StreamExt};
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_tokio_rpc_client::RpcClient; // Ensure this is the correct path to your RpcClient
use muxio_tokio_rpc_server::RpcServer; // Ensure this is the correct path to your RpcServer
use muxio_tokio_rpc_server::utils::{bind_tcp_listener_on_random_port, tcp_listener_to_host_port};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::sync::oneshot; // Needed for oneshot channel
use tokio::time::{Duration, timeout};
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

#[tokio::test]
async fn test_client_errors_on_connection_failure() {
    println!("[Test] Running test_client_errors_on_connection_failure");
    let (_, unused_port) = bind_tcp_listener_on_random_port().await.unwrap();
    println!("[Test] Listening on unused port: {}", unused_port);

    // Attempt to connect to an address that is not listening.
    let result = RpcClient::new("127.0.0.1", unused_port).await;
    println!("[Test] Connection attempt result: {:?}", result);

    // Assert that the connection attempt resulted in an error.
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::ConnectionRefused);
    println!("[Test] test_client_errors_on_connection_failure PASSED");
}

#[tokio::test]
async fn test_transport_state_change_handler() {
    println!("[Test] Running test_transport_state_change_handler");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();
    println!("[Test] Server listening on {}:{}", server_host, server_port);

    // This notify will be used to signal the *specific client handler* to shut down its connection.
    let client_connection_closer = Arc::new(Notify::new());
    let client_connection_closer_clone_for_test = client_connection_closer.clone();

    let server_task = tokio::spawn(async move {
        println!("[Server Task] Starting server accept loop. Waiting for one client.");
        if let Ok((socket, _addr)) = listener.accept().await {
            println!("[Server Task] Accepted client connection from: {}", _addr);
            if let Ok(ws_stream) = tokio_tungstenite::accept_async(socket).await {
                println!("[Server Task] WebSocket handshake complete for client.");
                let (mut ws_sender, mut ws_receiver) = ws_stream.split();

                // This clone is for the client handler task only.
                let notify_client_handler_shutdown = client_connection_closer.clone();

                let client_handler_task = tokio::spawn(async move {
                    println!("[Server Task Client Handler] Starting client handler loop.");
                    tokio::select! {
                        // Branch 1: Handle incoming messages (pong replies)
                        _ = async {
                            while let Some(msg_result) = ws_receiver.next().await {
                                match msg_result {
                                    Ok(WsMessage::Ping(data)) => {
                                        println!("[Server Task Client Handler] Received Ping, sending Pong.");
                                        let _ = ws_sender.send(WsMessage::Pong(data)).await;
                                    },
                                    Ok(msg) => { println!("[Server Task Client Handler] Received other message: {:?}", msg); },
                                    Err(e) => {
                                        println!("[Server Task Client Handler] WebSocket receive error: {:?}", e);
                                        break;
                                    }
                                }
                            }
                            println!("[Server Task Client Handler] Client handler receive loop finished naturally (e.g., client closed).");
                        } => {},
                        // Branch 2: Wait for explicit shutdown signal from the test
                        _ = notify_client_handler_shutdown.notified() => {
                            println!("[Server Task Client Handler] Received explicit shutdown signal from test.");
                        },
                    }
                    // This code runs when either branch completes (receive loop ends or shutdown notified)
                    println!("[Server Task Client Handler] Attempting to close WebSocket sender.");
                    let _ = ws_sender.close().await;
                    println!("[Server Task Client Handler] WebSocket sender closed.");
                });

                let _ = client_handler_task.await;
                println!("[Server Task] Client handler task completed/aborted.");
            } else {
                println!("[Server Task] WebSocket handshake failed for client.");
            }
        } else {
            println!("[Server Task] Listener accept failed.");
        }
        println!("[Server Task] Server accept loop finished.");
    });

    let received_states = Arc::new(Mutex::new(Vec::new()));
    let notify_disconnect = Arc::new(Notify::new());

    println!("[Test] Attempting to create RpcClient.");
    let client = RpcClient::new(&server_host.to_string(), server_port)
        .await
        .unwrap();
    println!("[Test] RpcClient created successfully.");

    let states_clone = received_states.clone();
    let notify_clone = notify_disconnect.clone();
    client
        .set_state_change_handler(move |state| {
            println!("[Test Handler] State Change Handler triggered: {:?}", state);
            if state == RpcTransportState::Disconnected {
                println!("[Test Handler] Notifying disconnect.");
                notify_clone.notify_one();
            }
            states_clone.lock().unwrap().push(state);
            println!(
                "[Test Handler] Current collected states: {:?}",
                states_clone.lock().unwrap()
            );
        })
        .await;
    println!("[Test] State change handler set.");

    // Give the client's internal tasks a moment to process the initial 'Connected' state.
    tokio::time::sleep(Duration::from_millis(50)).await;
    println!("[Test] Initial sleep after setting handler complete.");

    println!("[Test] Signaling server to close client connection...");
    client_connection_closer_clone_for_test.notify_one();

    // Wait for the disconnect handler to signal, with a timeout.
    println!("[Test] Waiting for disconnect notification...");
    let notification_result = timeout(Duration::from_secs(5), notify_disconnect.notified()).await;

    println!("[Test] Notification result: {:?}", notification_result);

    assert!(
        notification_result.is_ok(),
        "Test timed out waiting for disconnect notification. Collected states: {:?}",
        received_states.lock().unwrap()
    );

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
    println!("[Test] test_transport_state_change_handler PASSED");

    // Abort the main server task only after the client connection handling is done.
    // This cleans up the listener and any lingering server resources.
    server_task.abort();

    // A small sleep to allow the abort to fully propagate, although not strictly needed for test pass.
    tokio::time::sleep(Duration::from_millis(10)).await;
}

#[tokio::test]
async fn test_pending_requests_fail_on_disconnect() {
    println!("[Test] Running test_pending_requests_fail_on_disconnect");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();
    println!(
        "[Test] Server for pending requests test listening on {}:{}",
        server_host, server_port
    );

    let server_close_notify = Arc::new(Notify::new());
    let server_close_notify_clone = server_close_notify.clone();

    let server_task = tokio::spawn(async move {
        println!("[Server Task Pending] Waiting for client connection.");
        if let Ok((socket, _)) = listener.accept().await {
            println!("[Server Task Pending] Client connected. Attempting WebSocket handshake.");
            if let Ok(mut ws_stream) = tokio_tungstenite::accept_async(socket).await {
                println!(
                    "[Server Task Pending] WebSocket handshake complete. Waiting for first message from client."
                );
                // Server just sits here until notified to close.
                // It's crucial for the server to *not* try to read/process anything until signaled.
                server_close_notify_clone.notified().await; // Wait for signal to close
                println!(
                    "[Server Task Pending] Received close signal from test (server was waiting for message)."
                );

                println!("[Server Task Pending] Explicitly closing WebSocket stream.");
                let _ = ws_stream.close(None).await;
                println!("[Server Task Pending] WebSocket connection closed by server.");
            } else {
                println!("[Server Task Pending] WebSocket handshake failed.");
            }
        } else {
            println!("[Server Task Pending] Listener accept failed.");
        }
        println!("[Server Task Pending] Server task finished.");
    });

    println!("[Test] Attempting to create RpcClient for pending requests test.");
    // This correctly gets Arc<RpcClient> from RpcClient::new().
    let client: Arc<RpcClient> = RpcClient::new(&server_host.to_string(), server_port)
        .await
        .unwrap();
    println!("[Test] RpcClient created successfully.");
    tokio::time::sleep(Duration::from_millis(50)).await; // Give client time to connect
    println!("[Test] Client connected sleep complete.");

    // --- CRITICAL FIX: ENSURE RPC CALL IS PENDING BEFORE DISCONNECT ---

    // 1. Spawn the RPC call as a separate task.
    // This allows it to progress concurrently and become "pending".
    // We need to clone the Arc<RpcClient> for the spawned task.
    let client_clone_for_rpc_task = client.clone();
    let (tx_rpc_result, rx_rpc_result) = oneshot::channel(); // Channel to get result from spawned RPC task

    tokio::spawn(async move {
        println!("[RPC Task] Starting spawned RPC call.");
        // Make the call. This will interact with the dispatcher and its emit_fn.
        // It should become pending before the disconnect if timed correctly.
        let result = Echo::call(
            client_clone_for_rpc_task.as_ref(),
            b"this will fail".to_vec(),
        )
        .await;
        println!("[RPC Task] RPC call completed with result: {:?}", result);
        let _ = tx_rpc_result.send(result); // Send result back to main test thread
    });
    println!("[Test] RPC call spawned to run in background.");

    // 2. IMPORTANT: Give the RPC task ample time to become pending.
    // This sleep is crucial for the dispatcher to register the request.
    tokio::time::sleep(Duration::from_millis(300)).await; // Increased sleep for reliability.
    tokio::task::yield_now().await; // Give scheduler a chance to run all tasks.
    println!("[Test] RPC call should be pending in dispatcher now.");

    // 3. Now, signal the server to close the connection.
    // This will trigger the client's shutdown logic.
    println!("[Test] Signaling server to close connection.");
    server_close_notify.notify_one();

    // 4. Give client's shutdown logic time to run and cancel pending requests.
    tokio::time::sleep(Duration::from_millis(200)).await;
    tokio::task::yield_now().await;
    println!(
        "[Test] Sleep after server close signal complete (client should have processed disconnect)."
    );

    // 5. Await the result of the spawned RPC call task. It should be an error.
    println!("[Test] Waiting for spawned RPC call future to resolve (should be cancelled).");
    let result = timeout(Duration::from_secs(1), rx_rpc_result).await; // 1 sec timeout for resolution
    println!(
        "[Test] Spawned RPC call future resolution result: {:?}",
        result
    );

    assert!(
        result.is_ok(),
        "Test timed out waiting for RPC call to resolve. Result: {:?}",
        result
    );

    let rpc_result = result
        .unwrap()
        .expect("Oneshot channel should not be dropped");
    assert!(
        rpc_result.is_err(),
        "Expected the pending RPC call to fail, but it succeeded. Result: {:?}",
        rpc_result
    );

    let err_string = rpc_result.unwrap_err().to_string();
    println!("[Test] RPC error string: {}", err_string);
    // Error can be `ReadAfterCancel` or a general `Transport error` depending on propagation.
    assert!(
        err_string.contains("cancelled stream") || err_string.contains("Transport error"),
        "Error message should indicate that the request was cancelled due to a disconnect. Got: {}",
        err_string
    );
    println!("[Test] test_pending_requests_fail_on_disconnect PASSED");

    // --- END CRITICAL FIX ---

    server_task.abort();
    tokio::time::sleep(Duration::from_millis(10)).await;
}
