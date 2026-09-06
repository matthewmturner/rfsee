//! Serialize and deserialize the finished index at realistic size (the generated
//! bench-data/index.json; run `just generate-bench-data` first). The load half is
//! what every search pays today because the index is re-parsed from disk on each
//! search.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

fn bench_serde(c: &mut Criterion) {
    let index = benches::load_index();
    let load_path = benches::bench_index_path();
    let save_path = std::env::temp_dir().join("rfsee_bench_index.json");

    // Size-qualified so criterion baselines never compare across dataset sizes.
    let size = benches::bench_size_from_env();
    let mut group = c.benchmark_group(format!("index_serde_{size}"));
    group.sample_size(10);

    group.bench_function("save", |b| {
        b.iter(|| {
            let file = std::fs::File::create(&save_path).unwrap();
            simd_json::to_writer(file, &index).unwrap();
        })
    });

    group.bench_function("load", |b| {
        b.iter_batched(
            || std::fs::File::open(&load_path).unwrap(),
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
