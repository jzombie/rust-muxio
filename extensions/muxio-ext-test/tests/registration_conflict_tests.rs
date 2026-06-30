use muxio_ext_test::registration_conflict_tests;

registration_conflict_tests!(ws, muxio_tokio_rpc_client::RpcClient);
registration_conflict_tests!(ipc, muxio_tokio_rpc_ipc_client::RpcIpcClient);
registration_conflict_tests!(wasm, muxio_wasm_rpc_client::RpcWasmClient);
