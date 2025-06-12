use crate::endpoint::RpcPrebufferedHandler;
use std::collections::HashMap;

/// A trait that provides a generic, asynchronous interface for accessing a shared
/// `HashMap` of RPC handlers protected by a mutex.
///
/// This uses a closure-passing pattern to abstract over different mutex types
/// (e.g., `tokio::sync::Mutex` and `std::sync::Mutex`), allowing code to be
/// runtime-agnostic.
#[async_trait::async_trait]
pub trait WithHandlers: Send + Sync {
    /// Executes a closure with exclusive access to the handlers map.
    async fn with_handlers<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HashMap<u64, RpcPrebufferedHandler>) -> R + Send,
        R: Send;
}

// Only compile this block if the "tokio_support" feature is active.
#[cfg(feature = "tokio_support")]
#[async_trait::async_trait]
impl WithHandlers for tokio::sync::Mutex<HashMap<u64, RpcPrebufferedHandler>> {
    async fn with_handlers<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HashMap<u64, RpcPrebufferedHandler>) -> R + Send,
        R: Send,
    {
        let mut guard = self.lock().await;
        f(&mut guard)
    }
}
/// The implementation for the standard library's blocking mutex.
/// This is suitable for single-threaded environments like WASM.
#[async_trait::async_trait]
impl WithHandlers for std::sync::Mutex<HashMap<u64, RpcPrebufferedHandler>> {
    async fn with_handlers<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HashMap<u64, RpcPrebufferedHandler>) -> R + Send,
        R: Send,
    {
        let mut guard = self.lock().expect("Mutex was poisoned");
        f(&mut guard)
    }
}
