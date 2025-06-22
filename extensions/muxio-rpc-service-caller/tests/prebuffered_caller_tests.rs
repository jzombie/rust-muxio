use example_muxio_rpc_service_definition::prebuffered::Echo;
use futures::channel::mpsc;
use muxio::rpc::{
    RpcRequest,
    rpc_internals::{RpcHeader, RpcMessageType, RpcStreamEncoder, rpc_trait::RpcEmit},
};
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service_caller::{
    RpcServiceCallerInterface, WithDispatcher, error::RpcCallerError,
    prebuffered::RpcCallPrebuffered,
};
use std::{
    io,
    sync::{Arc, Mutex},
};

// --- Test Setup: Mock Implementations ---

// NOTE: This now needs to use the dynamic channel types from your new module.
// Make sure your lib.rs exports `dynamic_channel`.
use muxio_rpc_service_caller::dynamic_channel::{
    DynamicChannelType, DynamicReceiver, DynamicSender,
};

type SharedResponseSender = Arc<Mutex<Option<DynamicSender>>>;

/// A mock client that allows us to inject specific stream responses for testing.
#[derive(Clone)]
struct MockRpcClient {
    /// A shared structure to allow the test harness to provide the sender half of the
    /// mpsc channel to the mock implementation after it's been created.
    response_sender_provider: SharedResponseSender,
}

// Create a newtype wrapper around `Mutex<()>` to satisfy the orphan rule.
#[allow(dead_code)] // Ignores: field `0` is never read
struct MockDispatcherLock(Mutex<()>);

// Dummy implementation of the dispatcher trait for our newtype.
#[async_trait::async_trait]
impl WithDispatcher for MockDispatcherLock {
    async fn with_dispatcher<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut muxio::rpc::RpcDispatcher<'static>) -> R + Send,
        R: Send,
    {
        let mut dummy_dispatcher = muxio::rpc::RpcDispatcher::new();
        f(&mut dummy_dispatcher)
    }
}

#[async_trait::async_trait]
impl RpcServiceCallerInterface for MockRpcClient {
    type DispatcherLock = MockDispatcherLock;

    fn get_dispatcher(&self) -> Arc<Self::DispatcherLock> {
        Arc::new(MockDispatcherLock(Mutex::new(())))
    }

    fn get_emit_fn(&self) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        Arc::new(|_| {})
    }

    /// This is the core of the mock. It creates a new channel and gives the sender
    /// half back to the test harness via the shared `response_sender_provider`.
    async fn call_rpc_streaming(
        &self,
        _request: RpcRequest,
        dynamic_channel_type: DynamicChannelType,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            DynamicReceiver,
        ),
        io::Error,
    > {
        // The mock will now also respect the channel choice.
        let (tx, rx) = if dynamic_channel_type == DynamicChannelType::Unbounded {
            let (sender, receiver) = mpsc::unbounded();
            (
                DynamicSender::Unbounded(sender),
                DynamicReceiver::Unbounded(receiver),
            )
        } else {
            let (sender, receiver) = mpsc::channel(8);
            (
                DynamicSender::Bounded(sender),
                DynamicReceiver::Bounded(receiver),
            )
        };

        let dummy_encoder = {
            let dummy_header = RpcHeader {
                rpc_msg_type: RpcMessageType::Call,
                rpc_request_id: 0,
                rpc_method_id: 0,
                rpc_metadata_bytes: vec![],
            };
            let on_emit: Box<dyn RpcEmit + Send + Sync> = Box::new(|_| {});
            RpcStreamEncoder::new(0, 1024, &dummy_header, on_emit).unwrap()
        };

        *self.response_sender_provider.lock().unwrap() = Some(tx);

        Ok((dummy_encoder, rx))
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
        let error_payload = b"item does not exist".to_vec();
        sender.send_and_ignore(Err(RpcCallerError::RemoteError {
            payload: error_payload,
        }));
    });

    let request = RpcRequest {
        rpc_method_id: Echo::METHOD_ID,
        rpc_param_bytes: Some(vec![]),
        rpc_prebuffered_payload_bytes: None,
        is_finalized: true,
    };

    let (_, result) = client.call_rpc_buffered(request, decode_fn).await.unwrap();

    match result {
        Err(RpcCallerError::RemoteError { payload }) => {
            assert_eq!(payload, b"item does not exist");
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
        let error_message = "Method has panicked".to_string();
        sender.send_and_ignore(Err(RpcCallerError::RemoteSystemError(error_message)));
    });

    let result = Echo::call(&client, b"some input".to_vec()).await;

    assert!(result.is_err());
    let io_error = result.unwrap_err();
    assert_eq!(io_error.kind(), io::ErrorKind::Other);
    assert!(
        io_error
            .to_string()
            .contains("Remote system error: Method has panicked")
    );
}
