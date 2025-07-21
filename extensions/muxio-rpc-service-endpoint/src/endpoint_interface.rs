use super::endpoint_utils::process_single_prebuffered_request;
use super::{error::RpcServiceEndpointError, with_handlers_trait::WithHandlers};
use futures::future::join_all;
use muxio::rpc::{RpcDispatcher, rpc_internals::rpc_trait::RpcEmit};
use muxio_rpc_service::constants::DEFAULT_SERVICE_MAX_CHUNK_SIZE;
use std::{collections::hash_map::Entry, future::Future, marker::Send, sync::Arc};

#[async_trait::async_trait]
pub trait RpcServiceEndpointInterface<C>: Send + Sync
where
    C: Send + Sync + Clone + 'static,
{
    type HandlersLock: WithHandlers<C> + 'static;

    fn get_prebuffered_handlers(&self) -> Arc<Self::HandlersLock>;

    async fn register_prebuffered<F, Fut>(
        &self,
        method_id: u64, // <--- CHANGE BACK TO U64 HERE
        handler: F,
    ) -> Result<(), RpcServiceEndpointError>
    where
        F: Fn(C, Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>
            + Send
            + 'static,
    {
        self.get_prebuffered_handlers()
            .with_handlers(|handlers| match handlers.entry(method_id) {
                // `method_id` is now u64, matches HashMap key
                Entry::Occupied(_) => {
                    let err_msg =
                        format!("a handler for method ID {method_id} is already registered");
                    Err(RpcServiceEndpointError::Handler(err_msg.into()))
                }
                Entry::Vacant(entry) => {
                    let wrapped = move |ctx: C, bytes: Vec<u8>| {
                        Box::pin(handler(ctx, bytes))
                            as std::pin::Pin<Box<dyn Future<Output = _> + Send>>
                    };
                    entry.insert(Arc::new(wrapped));
                    Ok(())
                }
            })
            .await
    }

    async fn read_bytes<'a, E>(
        &self,
        dispatcher: &mut RpcDispatcher<'a>,
        context: C,
        bytes: &[u8],
        on_emit: E,
    ) -> Result<(), RpcServiceEndpointError>
    where
        E: RpcEmit + Send + Sync + Clone,
    {
        // --- Stage 1 ---
        // `dispatcher.read_bytes` returns Vec<u32>, so `id` is u32 here.
        let request_ids = dispatcher.read_bytes(bytes)?;
        let mut requests_to_process = Vec::new();
        for id in request_ids {
            if dispatcher.is_rpc_request_finalized(id).unwrap_or(false) {
                if let Some(req) = dispatcher.delete_rpc_request(id) {
                    requests_to_process.push((id, req)); // `id` is u32, `req.rpc_method_id` should be u32
                }
            }
        }

        if requests_to_process.is_empty() {
            return Ok(());
        }

        // --- Stage 2 ---
        let handlers_arc = self.get_prebuffered_handlers();
        let mut response_futures = Vec::new();
        for (request_id, request) in requests_to_process {
            // `request_id` is u32 here
            let handlers_arc_clone = handlers_arc.clone();
            let context_clone = context.clone();

            // Call to utility function.
            // Ensure process_single_prebuffered_request expects u32 for request_id.
            let future = process_single_prebuffered_request(
                handlers_arc_clone,
                context_clone,
                request_id, // This is u32
                request,
            );
            response_futures.push(future);
        }

        let responses = join_all(response_futures).await;

        // --- Stage 3 ---
        for response in responses {
            let _ = dispatcher.respond(response, DEFAULT_SERVICE_MAX_CHUNK_SIZE, on_emit.clone());
        }

        Ok(())
    }
}
