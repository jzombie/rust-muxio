pub mod endpoint_helpers;
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

/// Generate `#[tokio::test]` functions for streaming handler tests.
///
/// Tests:
/// - `test_streaming_handler_events_arrive` — send chunks, verify events
/// - `test_streaming_handler_method_not_found` — call unregistered method
#[macro_export]
macro_rules! streaming_handler_tests {
    ($module:ident, $transport:ty) => {
        mod $module {
            #[tokio::test]
            async fn test_streaming_handler_events_arrive() {
                let (client, _, events) =
                    <$transport as $crate::test_transport::TestTransport>::connect_for_streaming()
                        .await;
                $crate::test_suites::streaming_handler_events_arrive(client.as_ref(), &events)
                    .await;
            }

            #[tokio::test]
            async fn test_streaming_handler_method_not_found() {
                let (client, _, _) =
                    <$transport as $crate::test_transport::TestTransport>::connect_for_streaming()
                        .await;
                $crate::test_suites::streaming_handler_method_not_found(client.as_ref()).await;
            }
        }
    };
}

/// Generate `#[tokio::test]` functions for mpsc adapter tests.
///
/// Tests:
/// - `test_mpsc_open_channel` — client opens a channel, writes data, reads echoed response
/// - `test_mpsc_open_channel_method_not_found` — unregistered method doesn't hang
/// - `test_mpsc_channel_handler_s2c` — server-to-client streaming via channel handler
#[macro_export]
macro_rules! mpsc_adapter_tests {
    ($module:ident, $transport:ty) => {
        mod $module {
            use muxio_rpc_service_caller::RpcServiceCallerInterface;
            use std::sync::Arc;

            #[tokio::test]
            async fn test_mpsc_open_channel() {
                let (client, _, _) =
                    <$transport as $crate::test_transport::TestTransport>::connect_for_streaming()
                        .await;
                $crate::test_suites::mpsc_adapter_open_channel(client.as_ref()).await;
            }

            #[tokio::test]
            async fn test_mpsc_open_channel_method_not_found() {
                let (client, _, _) =
                    <$transport as $crate::test_transport::TestTransport>::connect_for_streaming()
                        .await;
                $crate::test_suites::mpsc_adapter_open_channel_method_not_found(client.as_ref())
                    .await;
            }

            #[tokio::test]
            async fn test_mpsc_channel_handler_s2c() {
                let (client, endpoint, handle) =
                    <$transport as $crate::test_transport::TestTransport>::connect_s2c().await;
                $crate::test_suites::mpsc_adapter_channel_handler_s2c(
                    client.clone(),
                    &*endpoint,
                    handle,
                )
                .await;
            }
        }
    };
}

/// Generate `#[tokio::test]` functions for complex concurrent tests.
///
/// Tests:
/// - `test_complex_concurrent_mixed` — multiple streams both directions with
///   interleaved prebuffered RPC calls
#[macro_export]
macro_rules! complex_concurrent_tests {
    ($module:ident, $transport:ty) => {
        mod $module {
            #[tokio::test]
            async fn test_complex_concurrent_mixed() {
                let (client, endpoint, handle) =
                    <$transport as $crate::test_transport::TestTransport>::connect_s2c().await;
                let label = <$transport as $crate::test_transport::TestTransport>::name();
                $crate::test_suites::complex_concurrent_mixed(client, &*endpoint, handle, label)
                    .await;
            }
        }
    };
}

/// Generate `#[tokio::test]` functions for registration conflict tests.
///
/// Tests:
/// - `test_duplicate_prebuffered_rejected` — same method_id twice
/// - `test_duplicate_streaming_rejected` — same method_id twice
/// - `test_cross_type_conflict_rejected` — prebuffered then streaming
#[macro_export]
macro_rules! registration_conflict_tests {
    ($module:ident, $transport:ty) => {
        mod $module {
            #[tokio::test]
            async fn test_duplicate_prebuffered_rejected() {
                let (_, endpoint) =
                    <$transport as $crate::test_transport::TestTransport>::connect().await;
                $crate::test_suites::duplicate_prebuffered_rejected(&*endpoint).await;
            }

            #[tokio::test]
            async fn test_duplicate_streaming_rejected() {
                let (_, endpoint) =
                    <$transport as $crate::test_transport::TestTransport>::connect().await;
                $crate::test_suites::duplicate_streaming_rejected(&*endpoint).await;
            }

            #[tokio::test]
            async fn test_cross_type_conflict_rejected() {
                let (_, endpoint) =
                    <$transport as $crate::test_transport::TestTransport>::connect().await;
                $crate::test_suites::cross_type_conflict_rejected(&*endpoint).await;
            }
        }
    };
}
