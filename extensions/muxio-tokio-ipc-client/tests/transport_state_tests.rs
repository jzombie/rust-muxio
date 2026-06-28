use example_muxio_rpc_service_definition::prebuffered::Echo;
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_rpc_service_caller::{RpcServiceCallerInterface, RpcTransportState};
use muxio_tokio_ipc_client::IpcClient;
use muxio_tokio_ipc_server::{IpcServer, RpcServiceEndpointInterface};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use tokio::time::{Duration, timeout};

fn temp_socket_name(name: &str) -> String {
    format!("muxio-ipc-test-{}", name)
}

#[tokio::test]
async fn test_client_errors_on_connection_failure() {
    let name = temp_socket_name("conn-fail");
    let result = IpcClient::new(&name).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::ConnectionRefused);
}

#[tokio::test]
async fn test_transport_state_change_handler() {
    let name = temp_socket_name("state-change");

    let server = IpcServer::new(None);
    let endpoint = server.endpoint();
    let _ = endpoint
        .register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
            Echo::decode_request(&request_bytes)?;
            let response_bytes = Echo::encode_response(b"ok".to_vec())?;
            Ok(response_bytes)
        })
        .await;

    let server_name = name.clone();
    tokio::spawn(async move {
        let _ = server.serve(&server_name).await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let received_states = Arc::new(Mutex::new(Vec::new()));
    let notify_disconnect = Arc::new(Notify::new());

    let client = IpcClient::new(&name).await.unwrap();

    let states_clone = received_states.clone();
    let notify_clone = notify_disconnect.clone();
    client
        .set_state_change_handler(move |state| {
            if state == RpcTransportState::Disconnected {
                notify_clone.notify_one();
            }
            states_clone.lock().unwrap().push(state);
        })
        .await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    drop(client);

    let notification_result = timeout(Duration::from_secs(5), notify_disconnect.notified()).await;
    assert!(
        notification_result.is_ok(),
        "Timed out waiting for disconnect notification. States: {:?}",
        received_states.lock().unwrap()
    );

    let final_states = received_states.lock().unwrap();
    assert_eq!(
        *final_states,
        vec![
            RpcTransportState::Connected,
            RpcTransportState::Disconnected
        ],
        "State change handler should have been called for both connect and disconnect. Actual: {:?}",
        *final_states
    );
}

#[tokio::test]
async fn test_pending_requests_fail_on_disconnect() {
    let name = temp_socket_name("pending-fail");

    let server_close = Arc::new(AtomicBool::new(false));
    let server_close_clone = server_close.clone();
    let server_name = name.clone();

    let server_task = tokio::spawn(async move {
        use interprocess::local_socket::{
            GenericNamespaced, ListenerOptions, ToNsName, tokio::prelude::*,
        };

        let ns_name = server_name.to_ns_name::<GenericNamespaced>().unwrap();
        let listener = ListenerOptions::new()
            .name(ns_name)
            .try_overwrite(true)
            .create_tokio()
            .unwrap();

        let _conn = listener.accept().await.unwrap();
        while !server_close_clone.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = IpcClient::new(&name).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let client_clone = client.clone();
    let (tx_result, rx_result) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = Echo::call(client_clone.as_ref(), b"will fail".to_vec()).await;
        let _ = tx_result.send(result);
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    server_close.store(true, Ordering::Relaxed);
    drop(client);

    let result = timeout(Duration::from_secs(3), rx_result)
        .await
        .expect("Timed out waiting for RPC call to resolve")
        .expect("Oneshot channel dropped");

    assert!(
        result.is_err(),
        "Expected the pending RPC call to fail, but it succeeded."
    );

    drop(server_task);
}
