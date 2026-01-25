use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use tokio::runtime::Runtime;

use tos_tck::bench::block_benchmark::{build_daemon, mine_one_block};

fn bench_block_mining(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime");
    c.bench_function("mine_empty_block", |b| {
        b.iter_batched(
            || rt.block_on(async { build_daemon(1).await.expect("build daemon") }),
            |daemon| {
                rt.block_on(async move {
                    mine_one_block(&daemon).await.expect("mine block");
                });
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_block_mining);
criterion_main!(benches);
