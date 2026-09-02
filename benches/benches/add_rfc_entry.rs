//! Per-document tokenization and term-frequency computation (`TfIdf::add_rfc_entry`)
//! on a real RFC body.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

const RFC_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/rfc_8124.txt");

fn bench_add_rfc_entry(c: &mut Criterion) {
    let contents = std::fs::read_to_string(RFC_PATH).unwrap();
    c.bench_function("add_rfc_entry", |b| {
        b.iter_batched(
            || {
                let entry = rfsee_tf_idf::RfcEntry {
                    number: 8124,
                    url: "https://rfsee.com/8124".to_string(),
                    title: "RFC 8124".to_string(),
                    content: Some(contents.clone()),
                };
                (rfsee_tf_idf::TfIdf::default(), entry)
            },
            |(mut index, entry)| index.add_rfc_entry(entry),
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, bench_add_rfc_entry);
criterion_main!(benches);
