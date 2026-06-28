use muxio_ext_test::prebuffered_roundtrip_tests;

prebuffered_roundtrip_tests!(ws, muxio_tokio_rpc_client::RpcClient);
prebuffered_roundtrip_tests!(ipc, muxio_tokio_ipc_client::IpcClient);
prebuffered_roundtrip_tests!(wasm, muxio_wasm_rpc_client::RpcWasmClient);
