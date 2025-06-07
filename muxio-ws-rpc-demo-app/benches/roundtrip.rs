use criterion::{Criterion, criterion_group, criterion_main};
use muxio_ws_rpc_demo_app::{
    RpcClient, RpcServer, add,
    service_definition::{Add, RpcApi},
};
use std::time::Duration;
use tokio::{net::TcpListener, runtime::Runtime};

fn bench_roundtrip(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Set up server and client once, outside of `b.iter`
    let (client, _server_task) = rt.block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = RpcServer::new();
        server
            .register(Add::METHOD_ID, |bytes| {
                let req = Add::decode_request(bytes).unwrap();
                let result = req.numbers.iter().sum();
                Add::encode_response(result)
            })
            .await;

        let server_task = tokio::spawn({
            let server = server;
            async move {
                let _ = server.serve_with_listener(listener).await;
            }
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        let client = RpcClient::new(&format!("ws://{}/ws", addr)).await;
        (client, server_task)
    });

    c.bench_function("rpc_add_roundtrip", |b| {
        b.iter(|| {
            rt.block_on(async {
                let result = add(&client, vec![1.0, 2.0, 3.0]).await.unwrap();
                assert_eq!(result, 6.0);
            });
        });
    });
}

criterion_group!(benches, bench_roundtrip);
criterion_main!(benches);
