//! Sync RPC server: protocol over owned blocking byte halves or the current
//! process stdio (one such pair), driven by std threads. Mirrors the client without the
//! spawn side: `start` begins pumping after handler registration and
//! `join_reader` blocks to EOF (the guest shape).

mod rpc_sync_server;

pub use rpc_sync_server::{RpcSyncServer, RpcSyncServerHandle, ServerSetup};
