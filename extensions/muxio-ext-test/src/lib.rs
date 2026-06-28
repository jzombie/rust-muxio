pub mod ipc_helpers;
pub mod test_assertions;
pub mod test_suites;
pub mod test_transport;
pub mod transports;
pub mod wasm_helpers;
pub mod ws_helpers;

/// Generate `#[tokio::test]` functions for all prebuffered roundtrip tests.
#[macro_export]
macro_rules! prebuffered_roundtrip_tests {
    ($module:ident, $transport:ty) => {
        mod $module {
            #[tokio::test]
            async fn test_success() {
                let (client, _) =
                    <$transport as $crate::test_transport::TestTransport>::connect().await;
                $crate::test_suites::roundtrip_success(client.as_ref()).await;
            }

            #[tokio::test]
            async fn test_error() {
                let (client, _endpoint) =
                    <$transport as $crate::test_transport::TestTransport>::connect().await;
                $crate::test_suites::roundtrip_error(client.as_ref()).await;
            }

            #[tokio::test]
            async fn test_large_payload() {
                let (client, _) =
                    <$transport as $crate::test_transport::TestTransport>::connect().await;
                $crate::test_suites::roundtrip_large_payload(client.as_ref()).await;
            }

            #[tokio::test]
            async fn test_method_not_found() {
                let (client, _) =
                    <$transport as $crate::test_transport::TestTransport>::connect().await;
                $crate::test_suites::roundtrip_method_not_found(client.as_ref()).await;
            }
        }
    };
}

/// Generate `#[tokio::test]` functions for server-to-client tests.
///
/// Current tests:
/// - `test_server_to_client_echo` — prebuffered request/response
/// - `test_server_to_client_streaming_echo` — chunked streaming
/// - `test_concurrent_bidirectional_streaming` — simultaneous streams both ways
#[macro_export]
macro_rules! server_to_client_tests {
    ($module:ident, $transport:ty) => {
        mod $module {
            #[tokio::test]
            async fn test_server_to_client_echo() {
                let (client, endpoint, handle) =
                    <$transport as $crate::test_transport::TestTransport>::connect_s2c().await;
                let label = <$transport as $crate::test_transport::TestTransport>::name();
                $crate::test_suites::server_to_client_echo(
                    client.as_ref(),
                    &*endpoint,
                    &handle,
                    label,
                )
                .await;
            }
            #[tokio::test]
            async fn test_server_to_client_streaming_echo() {
                let (client, endpoint, handle) =
                    <$transport as $crate::test_transport::TestTransport>::connect_s2c().await;
                let label = <$transport as $crate::test_transport::TestTransport>::name();
                $crate::test_suites::server_to_client_streaming_echo(
                    client.as_ref(),
                    &*endpoint,
                    &handle,
                    label,
                )
                .await;
            }

            #[tokio::test]
            async fn test_concurrent_bidirectional_streaming() {
                let (client, endpoint, handle) =
                    <$transport as $crate::test_transport::TestTransport>::connect_s2c().await;
                let label = <$transport as $crate::test_transport::TestTransport>::name();
                $crate::test_suites::concurrent_bidirectional_streaming(
                    client.clone(),
                    &*endpoint,
                    handle.clone(),
                    label,
                )
                .await;
            }

            #[tokio::test]
            async fn test_stream_small_messages_throughput() {
                let (client, endpoint, handle) =
                    <$transport as $crate::test_transport::TestTransport>::connect_s2c().await;
                let label = <$transport as $crate::test_transport::TestTransport>::name();
                $crate::test_suites::stream_small_messages_throughput(
                    client.as_ref(),
                    &*endpoint,
                    handle,
                    label,
                )
                .await;
            }
        }
    };
}

/// Generate `#[tokio::test]` functions for transport state tests.
#[macro_export]
macro_rules! transport_state_tests {
    ($module:ident, $transport:ty) => {
        mod $module {
            use muxio_rpc_service_caller::RpcServiceCallerInterface;
            use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;

            #[tokio::test]
            async fn test_connection_failure() {
                $crate::test_suites::transport_connection_failure(|| async {
                    <$transport as $crate::test_transport::TestTransport>::connect_fail().await
                })
                .await;
            }

            #[tokio::test]
            async fn test_state_change_handler() {
                let (client, _tx) =
                    <$transport as $crate::test_transport::TestTransport>::connect_with_disconnect(
                    )
                    .await;
                let states = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
                let notify = std::sync::Arc::new(tokio::sync::Notify::new());

                let s = states.clone();
                let n = notify.clone();
                client
                    .set_state_change_handler(move |state| {
                        if state == muxio_rpc_service_caller::RpcTransportState::Disconnected {
                            n.notify_one();
                        }
                        s.lock().unwrap().push(state);
                    })
                    .await;

                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                drop(client);

                let notified =
                    tokio::time::timeout(std::time::Duration::from_secs(5), notify.notified())
                        .await;
                assert!(notified.is_ok(), "Timed out waiting for disconnect");

                let final_states = states.lock().unwrap();
                assert_eq!(
                    *final_states,
                    vec![
                        muxio_rpc_service_caller::RpcTransportState::Connected,
                        muxio_rpc_service_caller::RpcTransportState::Disconnected,
                    ],
                    "Expected [Connected, Disconnected], got {:?}",
                    *final_states
                );
            }

            #[tokio::test]
            async fn test_pending_requests_fail_on_disconnect() {
                let (client, tx) =
                    <$transport as $crate::test_transport::TestTransport>::connect_with_disconnect(
                    )
                    .await;

                let client_clone = client.clone();
                let (tx_result, rx_result) = tokio::sync::oneshot::channel();
                tokio::spawn(async move {
                    let result = example_muxio_rpc_service_definition::prebuffered::Echo::call(
                        client_clone.as_ref(),
                        b"will fail".to_vec(),
                    )
                    .await;
                    let _ = tx_result.send(result);
                });

                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                drop(tx);

                let result = tokio::time::timeout(std::time::Duration::from_secs(3), rx_result)
                    .await
                    .expect("Timed out waiting for RPC call to resolve")
                    .expect("Oneshot channel dropped");

                assert!(
                    result.is_err(),
                    "Expected the pending RPC call to fail, but it succeeded."
                );
            }
        }
    };
}

// Keep old test_macros module for backwards compat if it exists
