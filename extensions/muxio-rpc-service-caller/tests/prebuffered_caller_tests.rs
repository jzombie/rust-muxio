use example_muxio_rpc_service_definition::prebuffered::Echo;
use futures::channel::mpsc;
use muxio::rpc::rpc_internals::{RpcHeader, RpcMessageType, RpcStreamEncoder, rpc_trait::RpcEmit};
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service_caller::{
    RpcServiceCallerInterface, WithDispatcher, error::RpcCallerError,
    prebuffered::RpcCallPrebuffered,
};
use std::{
    io,
    sync::{Arc, Mutex},
};
use tokio;

// --- Test Setup: Mock Implementations ---

/// A mock client that allows us to inject specific stream responses for testing.
#[derive(Clone)]
struct MockRpcClient {
    /// A shared structure to allow the test harness to provide the sender half of the
    /// mpsc channel to the mock implementation after it's been created.
    response_sender_provider: Arc<Mutex<Option<mpsc::Sender<Result<Vec<u8>, RpcCallerError>>>>>,
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
        _method_id: u64,
        _param_bytes: &[u8],
        _is_finalized: bool,
    ) -> Result<
        (
            RpcStreamEncoder<Box<dyn RpcEmit + Send + Sync>>,
            mpsc::Receiver<Result<Vec<u8>, RpcCallerError>>,
        ),
        io::Error,
    > {
        let (tx, rx) = mpsc::channel(8);

        // This dummy encoder is required by the trait signature but is not used in the tests.
        let dummy_encoder = {
            let dummy_header = RpcHeader {
                rpc_msg_type: RpcMessageType::Call,
                rpc_request_id: 0,
                rpc_method_id: 0,
                rpc_metadata_bytes: vec![],
            };
            // The `on_emit` closure must be correctly typed and boxed.
            let on_emit: Box<dyn RpcEmit + Send + Sync> = Box::new(|_| {});
            // Call the constructor with the correct arguments in the correct order,
            // based on the provided source code.
            RpcStreamEncoder::new(
                0,             // stream_id
                1024,          // max_chunk_size
                &dummy_header, // header
                on_emit,       // on_emit
            )
            .unwrap()
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

    // The data we expect to be echoed.
    let echo_payload = b"hello world".to_vec();

    // The decode function now expects a Vec<u8> and returns it directly.
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
            // Simulate the server echoing the payload back.
            sender.try_send(Ok(echo_payload)).unwrap();
        }
    });

    // Use the Echo service definition.
    let (_, result) = client
        .call_rpc_buffered(Echo::METHOD_ID, &echo_payload, decode_fn, true)
        .await
        .unwrap();

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
        sender
            .try_send(Err(RpcCallerError::RemoteError {
                payload: error_payload,
            }))
            .unwrap();
    });

    let (_, result) = client
        .call_rpc_buffered(Echo::METHOD_ID, &[], decode_fn, true)
        .await
        .unwrap();

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
        sender
            .try_send(Err(RpcCallerError::RemoteSystemError(error_message)))
            .unwrap();
    });

    // Use the Echo service with its RpcCallPrebuffered implementation.
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
