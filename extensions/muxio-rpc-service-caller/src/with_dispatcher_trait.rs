use muxio::rpc::RpcDispatcher;

/// A trait that provides a generic, asynchronous interface for accessing a shared
/// `RpcDispatcher` that may be protected by different kinds of mutexes.
///
/// ## The Problem This Solves
///
/// This trait solves the challenge of writing a single generic function that can
/// operate on an `RpcDispatcher` protected by either a `tokio::sync::Mutex` (for
/// native async code) or a `std::sync::Mutex` (for single-threaded WASM).
///
/// These two mutex types have incompatible lock guards (`tokio`'s is `Send`,
/// `std`'s is not), which prevents a simpler generic approach.
///
/// ## The Closure-Passing Pattern
///
/// Instead of trying to return a generic lock guard, this trait uses a
/// closure-passing pattern. The caller provides the work to be done via a
/// closure (`f`), and the implementation of this trait is responsible for:
///
/// 1. Acquiring the lock using its specific strategy (blocking or async).
/// 2. Executing the closure with a mutable reference to the locked data.
/// 3. Releasing the lock.
///
/// This encapsulates the locking logic and completely avoids the `Send` guard issue.
#[async_trait::async_trait]
pub trait WithDispatcher: Send + Sync {
    /// Executes a closure against the locked `RpcDispatcher`.
    ///
    /// # Type Parameters
    ///
    /// - `F`: A closure that takes `&mut RpcDispatcher` and is only called once.
    ///   It must be `Send` as the work may be moved to another thread.
    /// - `R`: The return type of the closure. It must be `Send` so the result can
    ///   be safely returned across `.await` points.
    async fn with_dispatcher<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut RpcDispatcher<'static>) -> R + Send,
        R: Send;
}

// This block is now only compiled when the `tokio_support` feature is enabled.
#[cfg(feature = "tokio_support")]
#[async_trait::async_trait]
impl WithDispatcher for tokio::sync::Mutex<RpcDispatcher<'static>> {
    async fn with_dispatcher<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut RpcDispatcher<'static>) -> R + Send,
        R: Send,
    {
        // Asynchronously acquires the lock without blocking the thread.
        let mut guard = self.lock().await;

        // Executes the provided work.
        f(&mut guard)
    }
}

// This implementation for std::sync::Mutex does not depend on tokio
// and is always available.
#[async_trait::async_trait]
impl WithDispatcher for std::sync::Mutex<RpcDispatcher<'static>> {
    async fn with_dispatcher<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut RpcDispatcher<'static>) -> R + Send,
        R: Send,
    {
        // This blocks the current thread, which is fine for the single-threaded WASM context.
        // In a Tokio context, this would ideally use `spawn_blocking`, but that would
        // bind this generic library to a specific runtime. This simple implementation
        // is correct for its intended use cases.
        // TODO: Don't use expect or unwrap
        let mut guard = self.lock().expect("Mutex was poisoned");

        // Executes the provided work.
        f(&mut guard)
    }
}
