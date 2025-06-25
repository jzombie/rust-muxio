use futures::{StreamExt, channel::mpsc};
use muxio::rpc::{
    RpcRequest,
    rpc_internals::{RpcHeader, RpcMessageType, RpcStreamEncoder, rpc_trait::RpcEmit},
};
use muxio_rpc_service_caller::{
    RpcServiceCallerInterface, RpcTransportState, WithDispatcher,
    dynamic_channel::{DynamicChannelType, DynamicReceiver, DynamicSender},
};
use std::{
    io,
    sync::{Arc, Mutex},
};

// --- Test Setup: Mock Implementations ---

// This type alias will hold our DynamicSender, allowing the test harness to
// provide either a bounded or unbounded sender to the mock.
type SharedResponseSender = Arc<Mutex<Option<DynamicSender>>>;

/// A mock client that allows us to inject specific stream responses for testing.
#[derive(Clone)]
struct MockRpcClient {
    response_sender_provider: SharedResponseSender,
}

// A dummy lock implementation is sufficient for these tests.
#[allow(dead_code)]
struct MockDispatcherLock(Mutex<()>);

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

    /// The mock implementation of `call_rpc_streaming`.
    /// It correctly uses the `use_unbounded_channel` flag to create the
    /// appropriate channel type for the test.
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
        let (tx, rx) = if dynamic_channel_type == DynamicChannelType::Unbounded {
            let (sender, receiver) = mpsc::unbounded();
            (
                DynamicSender::Unbounded(sender),
                DynamicReceiver::Unbounded(receiver),
            )
        } else {
            // Use a small buffer size to make it easy to test backpressure if needed.
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

        // Provide the sender half of the channel back to the test harness.
        *self.response_sender_provider.lock().unwrap() = Some(tx);

        Ok((dummy_encoder, rx))
    }

    /// A no-op implementation for the state change handler.
    /// This mock doesn't need to do anything with the handler, so the body is empty.
    fn set_state_change_handler(
        &self,
        _handler: impl Fn(RpcTransportState) + Send + Sync + 'static,
    ) {
        // No operation needed for the mock.
    }
}

// --- Unit Tests ---

/// A helper function to create a basic RpcRequest for testing.
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

    // Spawn a task that will act as the "server response".
    tokio::spawn({
        let expected_payload = expected_payload.clone();
        async move {
            // Wait until the mock client has created the sender for us.
            let mut sender = loop {
                if let Some(s) = sender_provider.lock().unwrap().take() {
                    break s;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            };
            // Send the mock payload.
            sender.send_and_ignore(Ok(expected_payload));
        }
    });

    // Make the RPC call, specifically requesting the BOUNDED channel.
    let (_encoder, mut stream) = client
        .call_rpc_streaming(create_test_request(), DynamicChannelType::Bounded)
        .await
        .unwrap();

    // Await the response from the stream and assert it's correct.
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

    // Spawn a task that will act as the "server response".
    tokio::spawn({
        let expected_payload = expected_payload.clone();
        async move {
            // Wait until the mock client has created the sender for us.
            let mut sender = loop {
                if let Some(s) = sender_provider.lock().unwrap().take() {
                    break s;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            };
            // Send the mock payload.
            sender.send_and_ignore(Ok(expected_payload));
        }
    });

    // Make the RPC call, specifically requesting the UNBOUNDED channel.
    let (_encoder, mut stream) = client
        .call_rpc_streaming(create_test_request(), DynamicChannelType::Unbounded)
        .await
        .unwrap();

    // Await the response from the stream and assert it's correct.
    let result = stream.next().await.unwrap().unwrap();
    assert_eq!(result, expected_payload);
}
