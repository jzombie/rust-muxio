mod client;
mod server;
mod service_definition;
use client::run_client;
use server::run_server;

#[tokio::main]
async fn main() {
    tokio::spawn(run_server("127.0.0.1:3000"));
    run_client("ws://127.0.0.1:3000/ws").await;
}
