use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use tokio::runtime::Runtime;

use tos_tck::bench::tps_benchmark::{build_daemon, submit_basic_transfers, test_pubkey};

fn bench_tps(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime");
    c.bench_function("tps_submit_and_mine_100", |b| {
        b.iter_batched(
            || rt.block_on(async { build_daemon(2).await.expect("build daemon") }),
            |daemon| {
                rt.block_on(async move {
                    let alice = test_pubkey(1);
                    let bob = test_pubkey(2);

                    submit_basic_transfers(&daemon, alice, bob, 100)
                        .await
                        .expect("submit transfers");

                    let _ = daemon.mine_block().await;
                });
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_tps);
criterion_main!(benches);
