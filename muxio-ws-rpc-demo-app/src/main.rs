mod client;
mod server;
mod service_definition;
use axum::{
    Router,
    extract::ConnectInfo,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use bitcode::{Decode, Encode};
use bytes::Bytes;
use client::{call_rpc, run_client};
use futures_util::{SinkExt, StreamExt};
use muxio::rpc::{
    RpcDispatcher, RpcRequest, RpcResponse, RpcResultStatus, rpc_internals::RpcStreamEvent,
};
use server::run_server;
use service_definition::{AddRequestParams, AddResponseParams};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::{
    net::TcpListener,
    sync::{
        Mutex,
        mpsc::{self, unbounded_channel},
        oneshot,
    },
    time::{Duration, sleep},
};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

#[tokio::main]
async fn main() {
    tokio::spawn(run_server("127.0.0.1:3000"));
    run_client("ws://127.0.0.1:3000/ws").await;
}
