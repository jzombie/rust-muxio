mod client;
mod server;
mod service_definition;
use client::RpcClient;
use muxio::rpc::RpcDispatcher;
use server::run_server;
use service_definition::{AddRequestParams, AddResponseParams};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

async fn add(
    dispatcher: Arc<Mutex<RpcDispatcher<'static>>>,
    tx: mpsc::UnboundedSender<WsMessage>,
    numbers: Vec<f64>,
) -> f64 {
    let payload = bitcode::encode(&AddRequestParams { numbers });
    let (_dispatcher, result) = RpcClient::call_rpc(
        dispatcher,
        tx,
        0x01,
        payload,
        |bytes| {
            let decoded: AddResponseParams = bitcode::decode(&bytes).unwrap();
            decoded.result
        },
        true,
    )
    .await;

    result
}

#[tokio::main]
async fn main() {
    tokio::spawn(run_server("127.0.0.1:3000"));

    // sleep(Duration::from_millis(300)).await;

    let rpc_client = RpcClient::new("ws://127.0.0.1:3000/ws").await;

    let result = add(rpc_client.dispatcher, rpc_client.tx, vec![1.0, 2.0, 3.0]).await;
    println!("Result from add(): {}", result);
}
