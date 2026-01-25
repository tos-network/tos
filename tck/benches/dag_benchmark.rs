use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

use tos_common::crypto::Hash;
use tos_common::difficulty::CumulativeDifficulty;
use tos_daemon::core::blockdag::{
    sort_ascending_by_cumulative_difficulty, sort_descending_by_cumulative_difficulty,
};
use tos_tck::bench::dag_benchmark::make_scores;

fn bench_dag_ordering(c: &mut Criterion) {
    c.bench_function("dag_ordering_sort_desc_100", |b| {
        b.iter_batched(
            || make_scores(100),
            |mut scores: Vec<(Hash, CumulativeDifficulty)>| {
                sort_descending_by_cumulative_difficulty(&mut scores);
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("dag_ordering_sort_asc_100", |b| {
        b.iter_batched(
            || make_scores(100),
            |mut scores: Vec<(Hash, CumulativeDifficulty)>| {
                sort_ascending_by_cumulative_difficulty(&mut scores);
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_dag_ordering);
criterion_main!(benches);
