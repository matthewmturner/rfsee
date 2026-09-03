//! Wall time for the full offline index build: per-document processing plus `finish`
//! (IDF + term scores) over the synthetic corpus. Set RFSEE_BENCH_DOCS to scale.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

fn bench_pipeline(c: &mut Criterion) {
    let docs = benches::doc_count_from_env();
    let corpus = benches::corpus(docs);
    let mut group = c.benchmark_group("index_pipeline");
    group.sample_size(10);
    group.bench_function(format!("build_and_finish_{docs}_docs"), |b| {
        b.iter_batched(
            || corpus.clone(),
            |entries| benches::build_index(entries),
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

criterion_group!(benches, bench_pipeline);
criterion_main!(benches);
