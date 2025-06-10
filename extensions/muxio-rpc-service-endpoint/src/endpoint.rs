use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;

type RpcPrebufferedHandler = Box<
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
    prebuffered_handlers: Arc<Mutex<HashMap<u64, RpcPrebufferedHandler>>>,
}
