//! Sync RPC client: protocol over owned blocking byte halves or a spawned
//! child process (whose piped stdio is one such pair), driven by std threads. No async runtime inside: the only
//! executor contact is `futures::executor::block_on` in the reader
//! thread, polling endpoint futures that never touch reactor-guarded
//! APIs. `tokio` appears solely as the `sync` feature for the
//! `Mutex` type the shared caller traits require.

mod rpc_sync_client;

pub use rpc_sync_client::RpcSyncClient;
