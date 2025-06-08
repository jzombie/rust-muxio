use criterion::{Criterion, criterion_group, criterion_main};
use example_muxio_ws_rpc_app::{RpcCallPrebuffered, RpcClient, RpcServer, service_definition::Add};
use futures::{StreamExt, stream::FuturesUnordered};
use muxio::rpc::optional_traits::{RpcRequestPrebuffered, RpcResponsePrebuffered};
use std::{hint::black_box, time::Duration};
use tokio::{join, net::TcpListener, runtime::Runtime};

fn bench_roundtrip(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Set up server + client once
    let (client, _server_task) = rt.block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = RpcServer::new();
        server
            .register(<Add as RpcRequestPrebuffered>::METHOD_ID, |bytes| {
                let req = Add::decode_request(bytes).unwrap();
                let result = req.iter().sum();
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

    // Benchmark only the actual call
    c.bench_function("rpc_add_roundtrip_batch_10", |b| {
        b.to_async(&rt).iter(|| async {
            let fut1 = Add::call(&client, vec![1.0, 2.0, 3.0]);
            let fut2 = Add::call(&client, vec![1.0, 2.0, 3.0]);
            let fut3 = Add::call(&client, vec![1.0, 2.0, 3.0]);
            let fut4 = Add::call(&client, vec![1.0, 2.0, 3.0]);
            let fut5 = Add::call(&client, vec![1.0, 2.0, 3.0]);
            let fut6 = Add::call(&client, vec![1.0, 2.0, 3.0]);
            let fut7 = Add::call(&client, vec![1.0, 2.0, 3.0]);
            let fut8 = Add::call(&client, vec![1.0, 2.0, 3.0]);
            let fut9 = Add::call(&client, vec![1.0, 2.0, 3.0]);
            let fut10 = Add::call(&client, vec![1.0, 2.0, 3.0]);

            let (r1, r2, r3, r4, r5, r6, r7, r8, r9, r10) =
                join!(fut1, fut2, fut3, fut4, fut5, fut6, fut7, fut8, fut9, fut10);

            black_box((
                r1.unwrap(),
                r2.unwrap(),
                r3.unwrap(),
                r4.unwrap(),
                r5.unwrap(),
                r6.unwrap(),
                r7.unwrap(),
                r8.unwrap(),
                r9.unwrap(),
                r10.unwrap(),
            ));
        });
    });

    c.bench_function("rpc_add_roundtrip_futures_unordered_batch_10", |b| {
        b.to_async(&rt).iter(|| async {
            let mut tasks = FuturesUnordered::new();

            for _ in 0..10 {
                tasks.push(Add::call(&client, vec![1.0, 2.0, 3.0]));
            }

            let mut results = Vec::with_capacity(10);
            while let Some(res) = tasks.next().await {
                results.push(res.unwrap());
            }

            black_box(results);
        });
    });
}

criterion_group!(benches, bench_roundtrip);
criterion_main!(benches);
