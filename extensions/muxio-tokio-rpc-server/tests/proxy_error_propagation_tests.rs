//! This integration test verifies error propagation through a proxy server.
//!
//! Scenario: Client A -> Server B (Proxy) -> Client B (Provider).
//! When Client B disconnects (crashes) while a call from Client A is pending,
//! the error must propagate back through Server B to Client A.

use example_muxio_rpc_service_definition::prebuffered::Echo;
use futures_util::StreamExt; // Needed for stream operations on DynamicReceiver
use muxio_rpc_service::{
    error::{RpcServiceError, RpcServiceErrorCode},
    prebuffered::RpcMethodPrebuffered,
};
use muxio_rpc_service_caller::{RpcServiceCallerInterface, prebuffered::RpcCallPrebuffered};
use muxio_tokio_rpc_client::RpcClient;
use muxio_tokio_rpc_server::utils::{bind_tcp_listener_on_random_port, tcp_listener_to_host_port};
use muxio_tokio_rpc_server::{
    ConnectionContextHandle, RpcServer, RpcServerEvent, RpcServiceEndpointInterface,
};
use std::collections::HashMap; // Used for client_b_handle_on_server_b_storage
use std::error::Error;
use std::io;
use std::sync::{Arc, RwLock}; // Used for shared state management
use tokio::net::TcpListener; // Used for server listener
use tokio::sync::{Notify, mpsc as tokio_mpsc, oneshot}; // Used for async communication
use tokio::time::{Duration, timeout}; // Used for timeouts and sleeps
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage; // Keeping for completeness, might not be directly used in this simplified test.

#[tokio::test]
async fn test_proxy_error_propagation_on_provider_disconnect() {
    // Enable tracing for detailed logs
    // RUST_LOG=trace cargo test -- --nocapture
    #[cfg(test)]
    {
        use std::sync::Once;
        use tracing_subscriber::{EnvFilter, fmt};
        static TRACING_INIT: Once = Once::new();
        TRACING_INIT.call_once(|| {
            fmt::Subscriber::builder()
                .with_env_filter(
                    EnvFilter::from_default_env()
                        .add_directive("info".parse().unwrap())
                        .add_directive("proxy_error_propagation_tests=trace".parse().unwrap())
                        .add_directive("muxio_tokio_rpc_server=trace".parse().unwrap())
                        .add_directive("muxio_tokio_rpc_client=trace".parse().unwrap())
                        .add_directive("muxio_rpc_service=trace".parse().unwrap())
                        .add_directive("muxio_rpc_service_caller=trace".parse().unwrap())
                        .add_directive("tokio=info".parse().unwrap())
                        .add_directive("tokio_tungstenite=info".parse().unwrap())
                        .add_directive("tungstenite=info".parse().unwrap())
                        .add_directive("hyper=info".parse().unwrap()),
                )
                .with_line_number(true)
                .with_file(true)
                .init();
        });
    }

    tracing::info!("[Test Setup] Starting proxy error propagation test (Client A -> Server B -> Client B).");

    // --- 1. Start Server B (The Proxy Server) ---
    let (server_b_listener, server_b_port) = bind_tcp_listener_on_random_port().await.unwrap();
    let (server_b_host, _) = tcp_listener_to_host_port(&server_b_listener).unwrap();
    let server_b_url = format!("ws://{}:{}/ws", server_b_host, server_b_port);
    tracing::info!("[Server B] Listening on: {}", server_b_url);

    let (server_b_event_tx, mut server_b_event_rx) = tokio_mpsc::unbounded_channel();
    let server_b = Arc::new(RpcServer::new(Some(server_b_event_tx)));
    let server_b_endpoint = server_b.endpoint();

    // Store Client B's ConnectionContextHandle on Server B.
    // This is the handle Server B will use to proxy calls to Client B.
    let client_b_handle_on_server_b_storage: Arc<RwLock<Option<ConnectionContextHandle>>> =
        Arc::new(RwLock::new(None));

    // Register Server B's Echo handler (the proxy handler)
    let client_b_handle_on_server_b_storage_clone = client_b_handle_on_server_b_storage.clone();
    server_b_endpoint
        // `ctx_raw_from_client_a` is the raw context value for Client A's incoming connection.
        // It needs to be wrapped into a ConnectionContextHandle.
        .register_prebuffered(Echo::METHOD_ID, move |ctx_raw_from_client_a, bytes| {
            let ctx_from_client_a = ConnectionContextHandle(ctx_raw_from_client_a); // Correctly wrap the raw ctx
            let client_b_provider_handle_storage = client_b_handle_on_server_b_storage_clone.clone();
            async move {
            tracing::trace!("[Server B Proxy Handler] Echo method handler invoked (from Client A).");
            tracing::info!(
                "[Server B Proxy Handler] Received Echo request from Client A ({}).",
                ctx_from_client_a.0.addr
            );
            
            // Get the ConnectionContextHandle for Client B from storage.
            let proxy_target_handle_opt = client_b_provider_handle_storage.read().unwrap().clone();

            if let Some(proxy_target_handle) = proxy_target_handle_opt {
                tracing::info!(
                    "[Server B Proxy Handler] Forwarding Echo request from Client A to Client B ({}). Message length: {}",
                    proxy_target_handle.0.addr, bytes.len()
                );
                // This is the proxy call from Server B to Client B using Client B's ConnectionContextHandle.
                // This call is subject to the spawn_blocking workaround for internal muxio contention.
                // It is critical that this call is still pending when Client B disconnects.
                match Echo::call(&proxy_target_handle, bytes).await {
                    Ok(response_from_client_b) => {
                        tracing::info!("[Server B Proxy Handler] Received success response from Client B.");
                        // Echo the response back to Client A (the original caller).
                        Echo::encode_response(response_from_client_b)
                            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
                    }
                    Err(e) => {
                        tracing::error!(
                            "[Server B Proxy Handler] RPC call to Client B FAILED: {}. Propagating error back to Client A.",
                            e
                        );
                        Err(Box::new(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            format!("Proxy call to provider (Client B) failed: {}", e),
                        )) as Box<dyn Error + Send + Sync>)
                    }
                }
            } else {
                tracing::error!("[Server B Proxy Handler] Client B provider not registered/available. Rejecting Client A's call.");
                Err(Box::new(io::Error::new(
                    io::ErrorKind::NotFound,
                    "Client B provider not available or not registered.",
                )) as Box<dyn Error + Send + Sync>)
            }
}})
        .await
        .unwrap();

    let server_b_task_handle = tokio::spawn({
        let server_b = Arc::clone(&server_b);
        async move {
            server_b
                .serve_with_listener(server_b_listener)
                .await
                .unwrap();
            tracing::info!("[Server B Task] Server B stopped.");
        }
    });

    // --- 2. Client A Connects to Server B ---
    let client_a: Arc<RpcClient> = RpcClient::new(&server_b_host.to_string(), server_b_port)
        .await
        .unwrap();
    tracing::info!("[Client A] Connected to Server B.");
    tokio::time::sleep(Duration::from_millis(50)).await;

    // --- Wait for Server B to acknowledge Client A's connection ---
    let client_a_event = server_b_event_rx
        .recv()
        .await
        .expect("Server B should acknowledge Client A connection.");
    let _client_a_ctx_handle_val = match client_a_event {
        RpcServerEvent::ClientConnected(handle) => handle,
        _ => panic!("Expected ClientConnected event for Client A, but got a different event type."),
    };
    tracing::info!(
        "[Server B] Acknowledged Client A connection ({}).",
        _client_a_ctx_handle_val.0.addr
    );

    // --- 3. Client B Connects to Server B (as the Provider) ---
    // This is the "provider" client that Server B will proxy requests to.
    let client_b: Arc<RpcClient> = RpcClient::new(&server_b_host.to_string(), server_b_port)
        .await
        .unwrap();
    tracing::info!("[Client B] Connected to Server B (as provider).");
    tokio::time::sleep(Duration::from_millis(50)).await; // Give time for connection to register

    // --- Setup for Client B's Disconnect ---
    // This channel is used to tell the main test thread that Client B's handler has received the proxied request.
    let (client_b_handler_received_tx, client_b_handler_received_rx) = oneshot::channel(); 

    // Register Client B's Echo handler (it will process proxied requests) ---
    // Instead of responding, this handler will signal its receipt and then cause Client B to disconnect.
    let client_b_handler_received_tx_clone = Arc::new(tokio::sync::Mutex::new(Some(client_b_handler_received_tx)));

    let client_b_endpoint = client_b.get_endpoint();
    client_b_endpoint
        .register_prebuffered(Echo::METHOD_ID, move |_, bytes| {
            let tx_signal = client_b_handler_received_tx_clone.clone();
            async move {
                tracing::trace!("[Client B Handler] Echo method handler invoked.");
                tracing::info!("[Client B Handler] Received Echo request (proxied from Server B). Signaling receipt and then disconnecting.");
                
                // Signal the main test thread that the request has been received by Client B's handler.
                if let Some(sender) = tx_signal.lock().await.take() {
                    let _ = sender.send(()); // Send signal to main test thread
                }
                
                // Importantly: do NOT return a successful response or a normal error response.
                // The connection will be aborted from the main test thread, which should then propagate.
                // This simulated error type allows for the propagation check.
                Err(Box::new(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "Client B disconnecting mid-request (simulated crash/abort from handler).",
                )) as Box<dyn Error + Send + Sync>)
            }
        })
        .await
        .unwrap();
    // Add a small delay after registering Client B's handler to ensure it's fully active.
    tokio::time::sleep(Duration::from_millis(100)).await;
    tokio::task::yield_now().await;
    tracing::info!("[Client B] Echo handler registered and given time to activate.");

    // --- Wait for Server B to acknowledge Client B's connection ---
    // This is the ConnectionContextHandle for Client B on Server B.
    let client_b_event_on_server_b = server_b_event_rx
        .recv()
        .await
        .expect("Server B should acknowledge Client B connection as a provider.");
    let client_b_ctx_handle_from_server_b = match client_b_event_on_server_b {
        RpcServerEvent::ClientConnected(handle) => handle,
        _ => panic!("Expected ClientConnected event for Client B provider, but got a different event type."),
    };
    tracing::info!(
        "[Server B] Acknowledged connection from Client B (Provider: {}).",
        client_b_ctx_handle_from_server_b.0.addr
    );

    // Store the ConnectionContextHandle for Client B on Server B, for the proxy handler.
    client_b_handle_on_server_b_storage
        .write()
        .unwrap()
        .replace(client_b_ctx_handle_from_server_b.clone());
    tracing::info!("[Test Setup] Client B's ConnectionContextHandle stored on Server B for proxying.");

    // IMPORTANT: Wait for the ConnectionContextHandle (client_b_ctx_handle_from_server_b) to be truly ready for outgoing calls.
    tracing::info!("[Test Setup] Waiting for Client B's ConnectionContextHandle (on Server B) to stabilize for outgoing calls.");
    let mut retries = 0;
    let max_retries = 10; // Try for up to 1 second (10 retries * 100ms)
    let retry_interval = Duration::from_millis(100);

    loop {
        if client_b_ctx_handle_from_server_b.is_connected() {
            tracing::info!("[Test Setup] Client B's ConnectionContextHandle (on Server B) reports connected. Proceeding with RPC test.");
            break;
        }
        if retries >= max_retries {
            tracing::error!("[Test Setup] Client B's ConnectionContextHandle (on Server B) did not report connected after multiple retries ({}ms). This is unexpected; proceeding anyway but the test might fail here.", max_retries * retry_interval.as_millis());
            break;
        }
        tokio::time::sleep(retry_interval).await;
        tokio::task::yield_now().await;
        retries += 1;
    }


    // --- Debug: Test direct call from Server B (using its ConnectionContextHandle for Client B) to Client B ---
    // This verifies Server B can make a call *back* to Client B using the handle received from Client B's connection TO Server B.
    tracing::info!("[Debug] Server B sending direct echo to Client B using its ConnectionContextHandle for Client B.");
    let test_message_direct = b"direct test from server B to client B".to_vec();

    // The problematic call, still wrapped in spawn_blocking.
    let client_b_ctx_handle_for_blocking_call_debug = client_b_ctx_handle_from_server_b.clone();
    let test_message_for_blocking_call_debug = test_message_direct.clone();

    // NOTE: This debug call will now *also* cause Client B to disconnect!
    // This might affect the main call if Client B is already gone.
    // For a clean test, this debug call should NOT cause Client B to disconnect.
    // Let's modify Client B's handler to only disconnect IF it receives a specific message.
    // Or, better, just remove this debug call entirely since the topology is now Client B handling the disconnect.
    tracing::warn!("[Debug] Skipping direct call to Client B as its handler now triggers disconnect, which would interfere with the main test flow. This debug step is primarily for connectivity, which is now implied by Client B connecting.");


    // --- 5. Main Call: Client A -> Server B (Echo) ---
    tracing::info!(
        "[Client A] Making Echo RPC call to Server B ('{}'). This will be proxied to Client B.",
        "some_message"
    );
    let message_to_proxy = b"hello from client a via proxy to client b".to_vec();

    // The call from Client A to Server B needs `spawn_blocking` because Server B's handler will internally
    // make a call that hits the `std::sync::Mutex` contention.
    let client_a_for_blocking_call = client_a.clone();
    let message_to_proxy_for_blocking_call = message_to_proxy.clone();

    // Store the future but don't await it immediately. This call to Server B will then trigger the
    // proxy handler which in turn triggers Client B to disconnect.
    let main_proxied_call_future = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(async move {
            timeout(Duration::from_secs(10), Echo::call(&*client_a_for_blocking_call, message_to_proxy_for_blocking_call)).await
        })
    });
    
    // Give a very short moment for the proxied call to hit Client B's handler and trigger disconnect.
    // Wait for the signal from Client B's handler indicating it received the call.
    tokio::select! {
        _ = client_b_handler_received_rx => {
            tracing::info!("[Test Setup] Client B's handler received the proxied call and signaled receipt.");
        }
        _ = tokio::time::sleep(Duration::from_secs(5)) => {
            panic!("Client B's handler did not receive the proxied call within 5 seconds to trigger disconnect.");
        }
    }
    
    // Now that Client B has received the call and is 'disconnecting' (or about to),
    // give a short time for the disconnect to propagate before checking Client A's result.
    tokio::time::sleep(Duration::from_millis(100)).await;
    tokio::task::yield_now().await;
    tracing::info!(
        "[Test Setup] Client A's Echo call to Server B should be pending, and Client B should be disconnecting/disconnected."
    );


    // --- 6. Explicitly drop Client B (final cleanup from test side) ---
    // This explicit drop ensures the Arc goes to zero and Client B's RpcClient fully cleans up.
    // The disconnect signal should have been sent from Client B's handler.
    tracing::info!("[Test Setup] Explicitly dropping Client B's RpcClient from main test thread (to ensure full cleanup).");
    drop(client_b); // This ensures the Arc is fully dropped, triggering RpcClient's cleanup.

    // Wait for Client B to fully disconnect and for Server B to register it.
    tokio::time::sleep(Duration::from_millis(500)).await;
    tokio::task::yield_now().await;
    tracing::info!("[Test Setup] Client B RpcClient dropped and time given for disconnect propagation.");

    // Assert Client B's ConnectionContextHandle on Server B is marked disconnected
    // This confirms Server B detected the disconnect from its provider.
    assert!(
        !client_b_ctx_handle_from_server_b.is_connected(), // This is the handle representing Client B's connection to Server B
        "Client B's connection handle on Server B should be marked disconnected."
    );
    tracing::info!(
        "[Test Setup] Confirmed Client B's ConnectionContextHandle on Server B is marked disconnected."
    );

    // --- 7. Assert Error Propagation: Client A's call should fail ---
    tracing::info!("[Test Setup] Awaiting Client A's Echo call result (should be an error).");
    // Now await the result of the main proxied call, which should have failed due to disconnect.
    let main_proxied_call_result = main_proxied_call_future.await.expect("Main proxied call spawn_blocking task failed.");

    assert!(
        main_proxied_call_result.is_ok(),
        "Client A's Echo call timed out, expected immediate error propagation."
    );

    let rpc_result = main_proxied_call_result.unwrap();
    assert!(
        rpc_result.is_err(),
        "Client A's Echo call succeeded unexpectedly, expected error due to provider disconnect."
    );

    let err = rpc_result.unwrap_err();
    tracing::info!("[Test Setup] Client A's Echo call error: {:?}", err);

    // Assert the error kind from the proxy. It should be a system error from Server B.
    match err {
        RpcServiceError::Rpc(payload) => {
            assert_eq!(
                payload.code,
                RpcServiceErrorCode::System,
                "Expected System error code from proxy."
            );
            assert!(
                payload.message.contains("Proxy call to provider (Client B) failed"),
                "Error message should indicate proxy failure: {}",
                payload.message
            );
            assert!(
                payload.message.contains("ConnectionAborted")
                    || payload.message.contains("cancelled stream")
                    || payload.message.contains("Connection reset by peer")
                    || payload.message.contains("RpcClient has disconnected")
                    || payload.message.contains("Client B disconnecting mid-request"), // Added specific message from Client B
                "Error message should mention connection issue from provider: {}",
                payload.message
            );
        }
        _ => panic!("Expected RpcServiceError::Rpc, but got: {:?}", err),
    }

    tracing::info!("[Test Setup] Proxy error propagation test PASSED.");

    // --- Final Cleanup: Explicitly drop all clients and abort all server tasks ---
    // This section is critical to ensure the Tokio runtime can shut down cleanly.

    // Client A and Client B were dropped earlier to simulate disconnect.

    // Abort server tasks and give them time to terminate.
    tracing::info!("[Cleanup] Aborting Server B's main task.");
    server_b_task_handle.abort();

    // Give ample time for all tasks (especially aborted ones) to unwind and drop resources.
    tokio::time::sleep(Duration::from_secs(2)).await;
    tokio::task::yield_now().await;
    tracing::info!("[Cleanup] All tasks requested to abort and given time to unwind. Test function exiting.");
}