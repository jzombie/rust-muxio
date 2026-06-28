use muxio_ext_test::transport_state_tests;

transport_state_tests!(ws, muxio_tokio_rpc_client::RpcClient);
transport_state_tests!(ipc, muxio_tokio_ipc_client::IpcClient);
transport_state_tests!(wasm, muxio_wasm_rpc_client::RpcWasmClient);
