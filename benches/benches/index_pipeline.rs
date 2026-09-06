//! Wall time for the full offline index build: per-document processing plus `finish`
//! (IDF + term scores) over the generated corpus. Run `just generate-bench-data`
//! first; scale the corpus with RFSEE_BENCH_DOCS at generation time.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

fn bench_pipeline(c: &mut Criterion) {
    let corpus = benches::load_corpus();
    let docs = corpus.len();
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
