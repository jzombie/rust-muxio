use crate::endpoint::RpcPrebufferedHandler;
use std::collections::HashMap;

/// A trait that provides a generic, asynchronous interface for accessing a shared
/// `HashMap` of RPC handlers protected by a mutex.
///
/// This trait is generic over a context type `C` that will be passed to handlers.
#[async_trait::async_trait]
pub trait WithHandlers<C>: Send + Sync
where
    C: Send + Sync + Clone + 'static,
{
    /// Executes a closure with exclusive access to the handlers map.
    async fn with_handlers<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HashMap<u64, RpcPrebufferedHandler<C>>) -> R + Send,
        R: Send;
}

// Only compile this block if the "tokio_support" feature is active.
#[cfg(feature = "tokio_support")]
#[async_trait::async_trait]
impl<C> WithHandlers<C> for tokio::sync::Mutex<HashMap<u64, RpcPrebufferedHandler<C>>>
where
    C: Send + Sync + Clone + 'static,
{
    async fn with_handlers<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HashMap<u64, RpcPrebufferedHandler<C>>) -> R + Send,
        R: Send,
    {
        let mut guard = self.lock().await;
        f(&mut guard)
    }
}

// This implementation is always available and suitable for WASM.
#[async_trait::async_trait]
impl<C> WithHandlers<C> for std::sync::Mutex<HashMap<u64, RpcPrebufferedHandler<C>>>
where
    C: Send + Sync + Clone + 'static,
{
    async fn with_handlers<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HashMap<u64, RpcPrebufferedHandler<C>>) -> R + Send,
        R: Send,
    {
        let mut guard = self.lock().expect("Mutex was poisoned");
        f(&mut guard)
    }
}
