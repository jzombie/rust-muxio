use muxio_ext_test::server_to_client_tests;

server_to_client_tests!(ws, muxio_tokio_rpc_client::RpcClient);
server_to_client_tests!(ipc, muxio_tokio_ipc_client::IpcClient);
server_to_client_tests!(wasm, muxio_wasm_rpc_client::RpcWasmClient);
