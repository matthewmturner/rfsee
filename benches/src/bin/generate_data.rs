//! Generates benchmark datasets into bench-data/ (gitignored), one subdirectory
//! per document count:
//!
//!   bench-data/<docs>/corpus.jsonl  - the synthetic corpus, one JSON RfcEntry per line
//!   bench-data/<docs>/index.json    - the finished TF-IDF index over that corpus
//!   bench-data/score-maps.json      - per-term score maps for combine_scores
//!                                     (size-independent, shared by all sizes)
//!
//! ```sh
//! cargo run --release -p benches --bin generate_data              # default sizes
//! cargo run --release -p benches --bin generate_data -- 500 5000  # explicit sizes
//! ```
//!
//! All benches load these files instead of generating in-process (select a size
//! with RFSEE_BENCH_SIZE), so bench runs skip corpus generation and every bench
//! sees byte-identical input.

use std::io::{BufWriter, Write};

use rfsee_tf_idf::TfIdf;

extern "C" fn noop_cb(_: *const std::ffi::c_char) {}

fn generate(docs: usize) -> std::io::Result<()> {
    let dir = benches::bench_data_dir().join(docs.to_string());
    std::fs::create_dir_all(&dir)?;

    eprintln!("generating corpus ({docs} docs) -> {}", dir.display());
    let mut corpus_file = BufWriter::new(std::fs::File::create(dir.join("corpus.jsonl"))?);
    let mut index = TfIdf::default();
    for entry in benches::corpus_stream(docs) {
        simd_json::to_writer(&mut corpus_file, &entry)?;
        corpus_file.write_all(b"\n")?;
        index.add_rfc_entry(entry);
    }
    corpus_file.flush()?;

    eprintln!("finishing index ({docs} docs)...");
    index.finish(noop_cb);
    index.save(&dir.join("index.json"));
    Ok(())
}

fn main() -> std::io::Result<()> {
    let sizes: Vec<usize> = std::env::args()
        .skip(1)
        .map(|a| {
            a.parse()
                .unwrap_or_else(|_| panic!("invalid doc count '{a}'"))
        })
        .collect();
    let sizes = if sizes.is_empty() {
        benches::DEFAULT_BENCH_SIZES.to_vec()
    } else {
        sizes
    };

    let root = benches::bench_data_dir();
    std::fs::create_dir_all(&root)?;

    for docs in &sizes {
        generate(*docs)?;
    }

    // Size-independent input for the combine_scores benches; write once at root.
    let maps = benches::score_maps(
        benches::SCORE_MAP_DOCS_PER_TERM,
        benches::SCORE_MAP_QUERY_TERMS,
    );
    simd_json::to_writer(std::fs::File::create(root.join("score-maps.json"))?, &maps)?;

    eprintln!("done");
    Ok(())
}
