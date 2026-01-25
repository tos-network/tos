use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use tokio::runtime::Runtime;

use tos_tck::bench::sync_benchmark::{build_pair, mine_blocks, sync_blocks};

fn bench_sync(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime");
    c.bench_function("sync_100_blocks", |b| {
        b.iter_batched(
            || {
                rt.block_on(async {
                    let (source, target) = build_pair(1).await.expect("build pair");
                    mine_blocks(&source, 100).await.expect("mine blocks");
                    (source, target)
                })
            },
            |(source, target)| {
                rt.block_on(async move {
                    sync_blocks(&source, &target, 100)
                        .await
                        .expect("sync blocks");
                });
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_sync);
criterion_main!(benches);
