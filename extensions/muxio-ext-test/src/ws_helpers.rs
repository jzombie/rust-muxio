use example_muxio_rpc_service_definition::prebuffered::{Add, Echo, Mult};
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service_endpoint::RpcServiceEndpointInterface;
use muxio_tokio_rpc_client::RpcClient;
use muxio_tokio_rpc_server::{
    RpcServer, RpcServerEvent, utils::tcp_listener_to_host_port,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::time::{Duration, sleep};

pub async fn register_standard_handlers<C>(endpoint: &impl RpcServiceEndpointInterface<C>)
where
    C: Send + Sync + Clone + 'static,
{
    endpoint
        .register_prebuffered(Add::METHOD_ID, |request_bytes, _ctx| async move {
            let request_params = Add::decode_request(&request_bytes)?;
            let sum = request_params.iter().sum();
            let response_bytes = Add::encode_response(sum)?;
            Ok(response_bytes)
        })
        .await
        .expect("Failed to register Add handler");
    endpoint
        .register_prebuffered(Mult::METHOD_ID, |request_bytes, _ctx| async move {
            let request_params = Mult::decode_request(&request_bytes)?;
            let product = request_params.iter().product();
            let response_bytes = Mult::encode_response(product)?;
            Ok(response_bytes)
        })
        .await
        .expect("Failed to register Mult handler");
    endpoint
        .register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
            let request_params = Echo::decode_request(&request_bytes)?;
            let response_bytes = Echo::encode_response(request_params)?;
            Ok(response_bytes)
        })
        .await
        .expect("Failed to register Echo handler");
}

pub async fn setup_ws_server() -> (Arc<RpcServer>, String, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();
    let server = Arc::new(RpcServer::new(None));
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.serve_with_listener(listener).await;
    });
    sleep(Duration::from_millis(200)).await;
    (server, server_host.to_string(), server_port)
}

pub async fn setup_ws_server_with_events(
) -> (
    Arc<RpcServer>,
    tokio::sync::mpsc::UnboundedReceiver<RpcServerEvent>,
    String,
    u16,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (server_host, server_port) = tcp_listener_to_host_port(&listener).unwrap();
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let server = Arc::new(RpcServer::new(Some(event_tx)));
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.serve_with_listener(listener).await;
    });
    sleep(Duration::from_millis(200)).await;
    (server, event_rx, server_host.to_string(), server_port)
}

pub async fn connect_ws_client(host: &str, port: u16) -> Arc<RpcClient> {
    RpcClient::new(host, port).await.unwrap()
}
