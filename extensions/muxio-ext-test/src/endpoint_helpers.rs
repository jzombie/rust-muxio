use example_muxio_rpc_service_definition::prebuffered::{Add, Echo, Mult};
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service_endpoint::RpcServiceEndpointInterface;
use std::error::Error;

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

pub async fn register_echo_handler<C>(endpoint: &impl RpcServiceEndpointInterface<C>)
where
    C: Send + Sync + Clone + 'static,
{
    endpoint
        .register_prebuffered(Echo::METHOD_ID, |request_bytes, _ctx| async move {
            Echo::encode_response(Echo::decode_request(&request_bytes)?)
                .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
        })
        .await
        .expect("Failed to register Echo handler");
}
