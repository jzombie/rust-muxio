use muxio_ext_test::streaming_handler_tests;

streaming_handler_tests!(ws, muxio_tokio_rpc_client::RpcClient);
streaming_handler_tests!(ipc, muxio_tokio_rpc_ipc_client::RpcIpcClient);
streaming_handler_tests!(wasm, muxio_wasm_rpc_client::RpcWasmClient);
