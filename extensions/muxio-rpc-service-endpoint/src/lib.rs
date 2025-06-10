// TODO: Migrate server handlers here, then do something like `impl RpcEndpoint for RpcClient``, etc.

// type RpcHandler = Box<dyn Fn(Vec<u8>) -> Vec<u8> + Send + Sync + 'static>;

// pub struct RpcEndpoint {
//     dispatcher: RpcDispatcher<'static>,
//     handlers: Arc<Mutex<HashMap<u64, RpcHandler>>>,
//     send: Box<dyn Fn(Bytes) + Send + Sync + 'static>,
// }
