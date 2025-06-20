use criterion::{Criterion, criterion_group, criterion_main};
use example_muxio_rpc_service_definition::prebuffered::Add;
use futures::{StreamExt, stream::FuturesUnordered};
use muxio_rpc_service::prebuffered::RpcMethodPrebuffered;
use muxio_rpc_service_caller::prebuffered::RpcCallPrebuffered;
use muxio_tokio_rpc_client::RpcClient;
use muxio_tokio_rpc_server::{RpcServer, RpcServiceEndpointInterface};
use std::{hint::black_box, sync::Arc, time::Duration};
use tokio::{net::TcpListener, runtime::Runtime};

fn bench_roundtrip(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Set up server + client once
    let (client, _server_task) = rt.block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = RpcServer::new();

        let endpoint = server.endpoint();

        endpoint
            .register_prebuffered(Add::METHOD_ID, |_, bytes| async move {
                let params = Add::decode_request(&bytes)?;
                let sum = params.iter().sum();
                let response_bytes = Add::encode_response(sum)?;
                Ok(response_bytes)
            })
            .await
            .ok();

        let server_task = tokio::spawn({
            let server = server;
            async move {
                let _ = Arc::new(server).serve_with_listener(listener).await;
            }
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        let client = RpcClient::new(&format!("ws://{}/ws", addr)).await;
        (client, server_task)
    });

    c.bench_function("rpc_add_roundtrip_futures_unordered_batch_10", |b| {
        b.to_async(&rt).iter(|| async {
            let mut tasks = FuturesUnordered::new();

            // Spawn n concurrent RPC calls to the Add method.
            // These futures are submitted all at once and polled concurrently.
            for _ in 0..10 {
                tasks.push(Add::call(&client, vec![1.0, 2.0, 3.0]));
            }

            let mut results = Vec::with_capacity(10);

            // Collect results as each RPC call completes, in completion order.
            // This loop allows early yielding of finished calls, ensuring true concurrency.
            while let Some(res) = tasks.next().await {
                results.push(res.unwrap());
            }

            // Prevent compiler from optimizing away the result
            black_box(results);
        });
    });

    c.bench_function("rpc_add_roundtrip_futures_unordered_singles", |b| {
        b.to_async(&rt).iter(|| async {
            let res = Add::call(&client, vec![1.0, 2.0, 3.0]).await;
            black_box(res.unwrap());
        });
    });
}

criterion_group!(benches, bench_roundtrip);
criterion_main!(benches);
