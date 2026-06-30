use muxio_ext_test::mpsc_adapter_tests;

mpsc_adapter_tests!(ws, muxio_tokio_rpc_client::RpcClient);
mpsc_adapter_tests!(ipc, muxio_tokio_rpc_ipc_client::RpcIpcClient);
mpsc_adapter_tests!(wasm, muxio_wasm_rpc_client::RpcWasmClient);
