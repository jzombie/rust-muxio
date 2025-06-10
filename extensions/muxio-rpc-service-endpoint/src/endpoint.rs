use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

pub type RpcPrebufferedHandler = Box<
    dyn Fn(
            Vec<u8>,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

pub struct RpcServiceEndpoint {
    // TODO: Privatize
    pub prebuffered_handlers: Arc<Mutex<HashMap<u64, RpcPrebufferedHandler>>>,
}

impl RpcServiceEndpoint {
    pub fn new() -> Self {
        Self {
            prebuffered_handlers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // TODO: Rename to `register_prebuffered`
    pub async fn register<F, Fut>(
        &self,
        method_id: u64,
        handler: F,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: Fn(Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>
            + Send
            + 'static,
    {
        // Acquire the lock. If it's poisoned, create a descriptive error message.
        let mut handlers = self.prebuffered_handlers.lock().await;

        // Use the entry API to atomically check and insert.
        match handlers.entry(method_id) {
            // If the key already exists, return an error.
            Entry::Occupied(_) => {
                let err_msg = format!(
                    "a handler for method ID {} is already registered",
                    method_id
                );
                Err(err_msg.into()) // .into() converts the String to the Box<dyn Error>
            }

            // If the key doesn't exist, insert the handler and return Ok.
            Entry::Vacant(entry) => {
                let wrapped = move |bytes: Vec<u8>| {
                    Box::pin(handler(bytes)) as Pin<Box<dyn Future<Output = _> + Send>>
                };
                entry.insert(Box::new(wrapped));
                Ok(())
            }
        }
    }
}
