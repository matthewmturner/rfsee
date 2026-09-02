//! Search latency in two modes:
//!
//! - warm: `search_index` on an already-parsed in-memory index (the algorithm only)
//! - cold: read + parse the index file, then search (what the CLI and FFI do on every
//!   search today)
//!
//! Queries cover a common term, a rare term, and a multi-term query.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

const COMMON_QUERY: &str = "term0";
const RARE_QUERY: &str = "term19999";
const MULTI_QUERY: &str = "term0 term1 term2 term3 term4";

fn bench_search(c: &mut Criterion) {
    let docs = benches::doc_count_from_env();
    let tfidf = benches::build_index(benches::corpus(docs));
    let path = std::env::temp_dir().join("rfsee_bench_search_index.json");
    tfidf.save(&path);

    let mut group = c.benchmark_group("search_index");

    for (name, query) in [
        ("warm_common", COMMON_QUERY),
        ("warm_rare", RARE_QUERY),
        ("warm_multi", MULTI_QUERY),
    ] {
        group.bench_function(name, |b| {
            b.iter_batched(
                || tfidf.index.clone(),
                |index| rfsee_tf_idf::search_index(query.to_string(), index),
                BatchSize::SmallInput,
            )
        });
    }

    group.bench_function("cold_common", |b| {
        b.iter_batched(
            || path.clone(),
            |path| {
                let file = std::fs::File::open(path).unwrap();
                let index: rfsee_tf_idf::Index = simd_json::from_reader(file).unwrap();
                rfsee_tf_idf::search_index(COMMON_QUERY.to_string(), index)
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, bench_search);
criterion_main!(benches);
