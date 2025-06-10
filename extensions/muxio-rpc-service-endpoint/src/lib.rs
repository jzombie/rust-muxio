// // TODO: Migrate server handlers here, then do something like `impl RpcEndpoint for RpcClient``, etc.

// // type RpcHandler = Box<dyn Fn(Vec<u8>) -> Vec<u8> + Send + Sync + 'static>;

// pub struct RpcEndpoint {
//     dispatcher: RpcDispatcher<'static>,
//     handlers: Arc<Mutex<HashMap<u64, RpcHandler>>>,
//     send: Box<dyn Fn(Bytes) + Send + Sync + 'static>,
// }

// impl RpcEndpoint {
//     pub fn receive_bytes(&self, bytes: &[u8]) {
//         // 1. Handle one-shot and fully-buffered calls (TODO: Extract to a separate handler)
//         {
//             let request_ids = match self.dispatcher.receive_bytes(&bytes) {
//                 Ok(ids) => ids,
//                 Err(e) => {
//                     eprintln!("Failed to decode incoming bytes: {e:?}");
//                     continue;
//                 }
//             };

//             for request_id in request_ids {
//                 if !dispatcher
//                     .is_rpc_request_finalized(request_id)
//                     .unwrap_or(false)
//                 {
//                     continue;
//                 }

//                 let Some(request) = dispatcher.delete_rpc_request(request_id) else {
//                     continue;
//                 };
//                 let Some(param_bytes) = &request.param_bytes else {
//                     continue;
//                 };

//                 let response = if let Some(handler) = handlers.lock().await.get(&request.method_id)
//                 {
//                     let encoded = handler(param_bytes.clone());
//                     RpcResponse {
//                         request_header_id: request_id,
//                         method_id: request.method_id,
//                         result_status: Some(RpcResultStatus::Success.value()),
//                         pre_buffered_payload_bytes: Some(encoded),
//                         is_finalized: true,
//                     }
//                 } else {
//                     RpcResponse {
//                         request_header_id: request_id,
//                         method_id: request.method_id,
//                         result_status: Some(RpcResultStatus::SystemError.value()),
//                         pre_buffered_payload_bytes: None,
//                         is_finalized: true,
//                     }
//                 };

//                 let tx_clone = tx.clone();
//                 dispatcher
//                     .respond(response, 1024, move |chunk| {
//                         let _ = tx_clone.send(Message::Binary(Bytes::copy_from_slice(chunk)));
//                     })
//                     .unwrap();
//             }
//         }
//     }
// }
