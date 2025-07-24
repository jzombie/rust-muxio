use example_muxio_rpc_service_definition::prebuffered::Echo;
use futures::channel::mpsc;
use muxio::rpc::{
    RpcDispatcher, RpcRequest,
    rpc_internals::{RpcHeader, RpcMessageType, RpcStreamEncoder, rpc_trait::RpcEmit},
};
use muxio_rpc_service::{
    error::{RpcServiceError, RpcServiceErrorCode, RpcServiceErrorPayload},
    prebuffered::RpcMethodPrebuffered,
};
use muxio_rpc_service_caller::{
    RpcServiceCallerInterface, RpcTransportState,
    dynamic_channel::{DynamicChannelType, DynamicReceiver, DynamicSender},
    prebuffered::RpcCallPrebuffered,
};
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as TokioMutex;

// --- Test Setup: Mock Implementation ---

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

        let emit_fn: Box<dyn RpcEmit + Send + Sync> = Box::new(|_: &[u8]| {});
        let dummy_encoder = RpcStreamEncoder::new(0, 1024, &dummy_header, emit_fn).unwrap();

        *self.response_sender_provider.lock().unwrap() = Some(tx);

        Ok((dummy_encoder, rx))
    }

    async fn set_state_change_handler(
        &self,
        _handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        // no-op
    }
}

// --- Unit Tests ---

#[tokio::test]
async fn test_buffered_call_success() {
    let sender_provider = Arc::new(Mutex::new(None));
    let client = MockRpcClient {
        response_sender_provider: sender_provider.clone(),
    };

    let echo_payload = b"hello world".to_vec();
    let decode_fn = |bytes: &[u8]| -> Vec<u8> { bytes.to_vec() };

    tokio::spawn({
        let echo_payload = echo_payload.clone();
        async move {
            let mut sender = loop {
                if let Some(s) = sender_provider.lock().unwrap().take() {
                    break s;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            };
            sender.send_and_ignore(Ok(echo_payload));
        }
    });

    let request = RpcRequest {
        rpc_method_id: Echo::METHOD_ID,
        rpc_param_bytes: Some(echo_payload.clone()),
        rpc_prebuffered_payload_bytes: None,
        is_finalized: true,
    };

    let (_, result) = client.call_rpc_buffered(request, decode_fn).await.unwrap();

    assert_eq!(result.unwrap(), echo_payload);
}

#[tokio::test]
async fn test_buffered_call_remote_error() {
    let sender_provider = Arc::new(Mutex::new(None));
    let client = MockRpcClient {
        response_sender_provider: sender_provider.clone(),
    };

    let decode_fn = |bytes: &[u8]| -> Vec<u8> { bytes.to_vec() };

    tokio::spawn(async move {
        let mut sender = loop {
            if let Some(s) = sender_provider.lock().unwrap().take() {
                break s;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        };

        sender.send_and_ignore(Err(RpcServiceError::Rpc(RpcServiceErrorPayload {
            code: RpcServiceErrorCode::Fail,
            message: "item does not exist".into(),
        })));
    });

    let request = RpcRequest {
        rpc_method_id: Echo::METHOD_ID,
        rpc_param_bytes: Some(vec![]),
        rpc_prebuffered_payload_bytes: None,
        is_finalized: true,
    };

    let (_, result) = client.call_rpc_buffered(request, decode_fn).await.unwrap();

    match result {
        Err(RpcServiceError::Rpc(err)) => {
            assert_eq!(err.code, RpcServiceErrorCode::Fail);
            assert_eq!(err.message, "item does not exist");
        }
        _ => panic!("Expected a RemoteError, but got something else."),
    }
}

#[tokio::test]
async fn test_prebuffered_trait_converts_error() {
    let sender_provider = Arc::new(Mutex::new(None));
    let client = MockRpcClient {
        response_sender_provider: sender_provider.clone(),
    };

    tokio::spawn(async move {
        let mut sender = loop {
            if let Some(s) = sender_provider.lock().unwrap().take() {
                break s;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        };

        sender.send_and_ignore(Err(RpcServiceError::Rpc(RpcServiceErrorPayload {
            code: RpcServiceErrorCode::System,
            message: "Method has panicked".into(),
        })));
    });

    let result = Echo::call(&client, b"some input".to_vec()).await;

    assert!(result.is_err());
    if let Err(RpcServiceError::Rpc(err)) = result {
        assert_eq!(err.code, RpcServiceErrorCode::System);
        assert_eq!(err.message, "Method has panicked");
    } else {
        panic!("Expected Rpc error");
    }
}
