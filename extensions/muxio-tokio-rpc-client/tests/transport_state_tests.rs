use example_muxio_rpc_service_definition::prebuffered::Echo; // Assuming this is defined
use futures_util::SinkExt;
use futures_util::StreamExt;
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_tokio_rpc_client::RpcClient; // Ensure this is the correct path to your RpcClient
use muxio_tokio_rpc_server::RpcServer; // Ensure this is the correct path to your RpcServer
use muxio_tokio_rpc_server::utils::{bind_tcp_listener_on_random_port, tcp_listener_to_host_port};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::Notify;
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
                                        break; // Break loop on error
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
                    let _ = ws_sender.close().await; // Close the sender half explicitly
                    println!("[Server Task Client Handler] WebSocket sender closed.");
                });

                // The main server_task does NOT need to wait on `notified()` here.
                // It just needs to ensure `client_handler_task` runs and doesn't get dropped.
                // We'll join this task later (or let the test's final `server_task.abort()` clean it up).
                let _ = client_handler_task.await; // Wait for the client handler to finish or be aborted.
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
    client_connection_closer_clone_for_test.notify_one(); // Signal the client handler task directly.

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
    println!("[Test] Final states collected: {:?}", *final_states);

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
                tokio::select! {
                    msg_opt = ws_stream.next() => {
                        println!("[Server Task Pending] Received message from client: {:?}", msg_opt);
                        tokio::time::sleep(Duration::from_millis(10)).await; // Small pause if message received
                    },
                    _ = server_close_notify_clone.notified() => {
                        println!("[Server Task Pending] Received early close signal from test (server was waiting for message).");
                    }
                }

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
    let client = RpcClient::new(&server_host.to_string(), server_port)
        .await
        .unwrap();
    println!("[Test] RpcClient created successfully.");
    tokio::time::sleep(Duration::from_millis(50)).await; // Give client time to connect
    println!("[Test] Client connected sleep complete.");

    // ### CRITICAL FIX: Initiate the RPC call *before* the disconnection. ###
    println!("[Test] Initiating RPC call that should become pending.");
    // Directly await the call future here, instead of spawning it.
    // This removes the Send requirement for the future.
    let call_future = Echo::call(client.as_ref(), b"this will hang".to_vec());

    // Give enough time for the RPC request to be fully registered with the dispatcher
    // and potentially sent to the server.
    tokio::time::sleep(Duration::from_millis(100)).await; // Increased sleep to ensure RPC is pending
    tokio::task::yield_now().await; // Ensure scheduler runs tasks to process the RPC initiation
    println!("[Test] RPC call initiated and should now be pending in dispatcher.");

    // Now, signal the server to close the connection.
    println!("[Test] Signaling server to close connection.");
    server_close_notify.notify_one();

    // Give the client's receive loop and shutdown_async task time to complete.
    // This is where `fail_all_pending_requests` should now find the pending request.
    tokio::time::sleep(Duration::from_millis(200)).await; // Allow time for client to detect close and run shutdown
    tokio::task::yield_now().await; // Ensure scheduler runs dispatcher's cancellation
    println!(
        "[Test] Sleep after server close signal complete (client should have processed disconnect)."
    );

    println!("[Test] Waiting for RPC call future to resolve (should be cancelled).");
    // Now, `timeout` wraps the `call_future` directly.
    let result = timeout(Duration::from_secs(1), call_future).await; // 1 second should be plenty if cancellation works
    println!("[Test] RPC call future resolution result: {:?}", result);

    assert!(
        result.is_ok(),
        "Test timed out, the future did not resolve. Result: {:?}",
        result
    );

    let rpc_result = result.unwrap();
    assert!(
        rpc_result.is_err(),
        "Expected the pending RPC call to fail, but it succeeded. Result: {:?}",
        rpc_result
    );

    let err_string = rpc_result.unwrap_err().to_string();
    println!("[Test] RPC error string: {}", err_string);
    assert!(
        err_string.contains("cancelled stream"), // This is the error from FrameDecodeError::ReadAfterCancel
        "Error message should indicate that the request was cancelled due to a disconnect. Got: {}",
        err_string
    );
    println!("[Test] test_pending_requests_fail_on_disconnect PASSED");

    // Clean up the server task at the end.
    server_task.abort();
    tokio::time::sleep(Duration::from_millis(10)).await;
}
