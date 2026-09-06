//! `combine_scores` in isolation: merging per-term score maps and sorting every
//! matching document. This is the baseline for a top-N cap, which should replace the
//! full sort with a partial selection.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

fn bench_combine(c: &mut Criterion) {
    let maps = benches::score_maps(
        benches::SCORE_MAP_DOCS_PER_TERM,
        benches::SCORE_MAP_QUERY_TERMS,
    );
    c.bench_function("combine_scores", |b| {
        b.iter_batched(
            || maps.clone(),
            |maps| rfsee_tf_idf::combine_scores(maps),
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, bench_combine);
criterion_main!(benches);
