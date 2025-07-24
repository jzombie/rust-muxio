use futures::{StreamExt, channel::mpsc};
use muxio::rpc::{
    RpcDispatcher, RpcRequest,
    rpc_internals::{RpcHeader, RpcMessageType, RpcStreamEncoder, rpc_trait::RpcEmit},
};
use muxio_rpc_service::error::RpcServiceError;
use muxio_rpc_service_caller::{
    RpcServiceCallerInterface, RpcTransportState,
    dynamic_channel::{DynamicChannelType, DynamicReceiver, DynamicSender},
};
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as TokioMutex;

// --- Test Setup: Mock Implementations ---

type SharedResponseSender = Arc<Mutex<Option<DynamicSender>>>;

#[derive(Clone)]
struct MockRpcClient {
    response_sender_provider: SharedResponseSender,
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for MockRpcClient {
    fn get_dispatcher(&self) -> Arc<TokioMutex<RpcDispatcher<'static>>> {
        Arc::new(TokioMutex::new(RpcDispatcher::new()))
    }

    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        Arc::new(|_| {})
    }

    async fn call_rpc_streaming(
        &self,
        _request: RpcRequest,
        dynamic_channel_type: DynamicChannelType,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            DynamicReceiver,
        ),
        RpcServiceError,
    > {
        let (tx, rx) = match dynamic_channel_type {
            DynamicChannelType::Unbounded => {
                let (sender, receiver) = mpsc::unbounded();
                (
                    DynamicSender::Unbounded(sender),
                    DynamicReceiver::Unbounded(receiver),
                )
            }
            DynamicChannelType::Bounded => {
                let (sender, receiver) = mpsc::channel(8);
                (
                    DynamicSender::Bounded(sender),
                    DynamicReceiver::Bounded(receiver),
                )
            }
        };

        let dummy_header = RpcHeader {
            rpc_msg_type: RpcMessageType::Call,
            rpc_request_id: 0,
            rpc_method_id: 0,
            rpc_metadata_bytes: vec![],
        };

        let on_emit: Box<dyn RpcEmit + Send + Sync> = Box::new(|_| {});
        let dummy_encoder = RpcStreamEncoder::new(0, 1024, &dummy_header, on_emit).unwrap();

        *self.response_sender_provider.lock().unwrap() = Some(tx);

        Ok((dummy_encoder, rx))
    }

    async fn set_state_change_handler(
        &self,
        _handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        // No-op for test
    }
}

// --- Unit Tests ---

fn create_test_request() -> RpcRequest {
    RpcRequest {
        rpc_method_id: 1,
        rpc_param_bytes: None,
        rpc_prebuffered_payload_bytes: None,
        is_finalized: true,
    }
}

#[tokio::test]
async fn test_dynamic_channel_bounded() {
    let sender_provider = Arc::new(Mutex::new(None));
    let client = MockRpcClient {
        response_sender_provider: sender_provider.clone(),
    };

    let expected_payload = b"data from bounded channel".to_vec();

    tokio::spawn({
        let expected_payload = expected_payload.clone();
        async move {
            let mut sender = loop {
                if let Some(s) = sender_provider.lock().unwrap().take() {
                    break s;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            };
            sender.send_and_ignore(Ok(expected_payload));
        }
    });

    let (_encoder, mut stream) = client
        .call_rpc_streaming(create_test_request(), DynamicChannelType::Bounded)
        .await
        .unwrap();

    let result = stream.next().await.unwrap().unwrap();
    assert_eq!(result, expected_payload);
}

#[tokio::test]
async fn test_dynamic_channel_unbounded() {
    let sender_provider = Arc::new(Mutex::new(None));
    let client = MockRpcClient {
        response_sender_provider: sender_provider.clone(),
    };

    let expected_payload = b"data from unbounded channel".to_vec();

    tokio::spawn({
        let expected_payload = expected_payload.clone();
        async move {
            let mut sender = loop {
                if let Some(s) = sender_provider.lock().unwrap().take() {
                    break s;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            };
            sender.send_and_ignore(Ok(expected_payload));
        }
    });

    let (_encoder, mut stream) = client
        .call_rpc_streaming(create_test_request(), DynamicChannelType::Unbounded)
        .await
        .unwrap();

    let result = stream.next().await.unwrap().unwrap();
    assert_eq!(result, expected_payload);
}
