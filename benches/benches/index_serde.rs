//! Serialize (`save`) and deserialize the finished index at realistic size. The load
//! half is what every `:RFSee` search pays today because the index is re-parsed from
//! disk on each search.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

fn bench_serde(c: &mut Criterion) {
    let docs = benches::doc_count_from_env();
    let index = benches::build_index(benches::corpus(docs));
    let path = std::env::temp_dir().join("rfsee_bench_index.json");

    let mut group = c.benchmark_group("index_serde");
    group.sample_size(10);

    group.bench_function("save", |b| b.iter(|| index.save(&path)));

    group.bench_function("load", |b| {
        b.iter_batched(
            || std::fs::File::open(&path).unwrap(),
            |file| {
                let index: rfsee_tf_idf::Index = simd_json::from_reader(file).unwrap();
                std::hint::black_box(index);
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, bench_serde);
criterion_main!(benches);
