//! `combine_scores` in isolation: merging per-term score maps and sorting every
//! matching document. This is the baseline for a top-N cap, which should replace the
//! full sort with a partial selection.

use std::collections::HashMap;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

/// Documents per term map. Common terms in the real index appear in nearly all docs.
const DOCS_PER_TERM: usize = 10_000;
/// Number of terms in the query being combined.
const QUERY_TERMS: usize = 5;

fn score_maps() -> Vec<HashMap<i32, i32>> {
    (0..QUERY_TERMS)
        .map(|term| {
            (0..DOCS_PER_TERM as i32)
                // Shift each term's doc ids so maps partially overlap, and vary scores.
                .map(|doc| (doc + term as i32, (doc * 31 + term as i32) % 1_000_000))
                .collect()
        })
        .collect()
}

fn bench_combine(c: &mut Criterion) {
    let maps = score_maps();
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
