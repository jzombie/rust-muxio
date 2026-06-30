use muxio_ext_test::complex_concurrent_tests;

complex_concurrent_tests!(ws, muxio_tokio_rpc_client::RpcClient);
complex_concurrent_tests!(ipc, muxio_tokio_rpc_ipc_client::RpcIpcClient);
complex_concurrent_tests!(wasm, muxio_wasm_rpc_client::RpcWasmClient);
