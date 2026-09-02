//! Memory profiler for the index-building pipeline.
//!
//! This is not a criterion benchmark: allocation counting requires a process-wide
//! `#[global_allocator]`, so it runs as a standalone binary:
//!
//! ```sh
//! cargo run --release -p benches -- [buffered|streaming]
//! RFSEE_BENCH_DOCS=2000 cargo run --release -p benches -- streaming
//! ```
//!
//! `buffered` materializes the entire corpus in memory before processing, mirroring
//! `par_load_rfcs` today. `streaming` generates and processes one document at a time,
//! mirroring the pipeline after fetched documents are consumed as they arrive.
//!
//! Reports allocation count and peak live heap bytes (via the counting allocator)
//! plus total process peak RSS (VmHWM from /proc/self/status).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use benches::{build_index, corpus, corpus_stream, doc_count_from_env};

/// A [`GlobalAlloc`] wrapper that counts allocations and tracks live/peak heap bytes.
struct CountingAlloc {
    allocs: AtomicUsize,
    live_bytes: AtomicUsize,
    peak_bytes: AtomicUsize,
}

impl CountingAlloc {
    const fn new() -> Self {
        Self {
            allocs: AtomicUsize::new(0),
            live_bytes: AtomicUsize::new(0),
            peak_bytes: AtomicUsize::new(0),
        }
    }
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            self.allocs.fetch_add(1, Ordering::Relaxed);
            let live = self.live_bytes.fetch_add(layout.size(), Ordering::Relaxed)
                + layout.size();
            self.peak_bytes.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.live_bytes.fetch_sub(layout.size(), Ordering::Relaxed);
        System.dealloc(ptr, layout);
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc::new();

/// Peak RSS (VmHWM) of this process in KiB. Linux only; returns 0 elsewhere.
fn peak_rss_kib() -> u64 {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return 0;
    };
    status
        .lines()
        .find(|l| l.starts_with("VmHWM:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "buffered".to_string());
    let docs = doc_count_from_env();

    let allocs_start = ALLOC.allocs.load(Ordering::Relaxed);
    let peak_heap_start = ALLOC.peak_bytes.load(Ordering::Relaxed);
    let rss_start_kib = peak_rss_kib();

    let index = match mode.as_str() {
        "buffered" => build_index(corpus(docs)),
        "streaming" => build_index(corpus_stream(docs)),
        other => {
            eprintln!("unknown mode '{other}', expected 'buffered' or 'streaming'");
            std::process::exit(2);
        }
    };

    // Keep the index alive until after the final measurements.
    let terms = std::hint::black_box(&index.index.term_scores).len();

    let allocs = ALLOC.allocs.load(Ordering::Relaxed) - allocs_start;
    let peak_heap = ALLOC.peak_bytes.load(Ordering::Relaxed) - peak_heap_start;
    let rss_end_kib = peak_rss_kib();

    println!("mode={mode}");
    println!("docs={docs}");
    println!("index_terms={terms}");
    println!("alloc_count={allocs}");
    println!("peak_heap_bytes={peak_heap}");
    println!("peak_rss_kib_start={rss_start_kib}");
    println!("peak_rss_kib_end={rss_end_kib}");

    let record = LogRecord {
        timestamp: unix_timestamp_ms(),
        git_sha: git_sha(),
        mode: &mode,
        docs,
        index_terms: terms,
        alloc_count: allocs,
        peak_heap_bytes: peak_heap,
        peak_rss_kib: rss_end_kib,
    };
    match append_log(&record) {
        Ok(path) => println!("logged_to={}", path.display()),
        Err(e) => eprintln!("failed to write memory profile log: {e}"),
    }
}

struct LogRecord<'a> {
    timestamp: u64,
    git_sha: String,
    mode: &'a str,
    docs: usize,
    index_terms: usize,
    alloc_count: usize,
    peak_heap_bytes: usize,
    peak_rss_kib: u64,
}

const LOG_HEADER: &str =
    "timestamp_ms,git_sha,mode,docs,index_terms,alloc_count,peak_heap_bytes,peak_rss_kib";

/// Default log path, anchored to this crate so cwd doesn't matter. Override with
/// the RFSEE_MEM_LOG env var.
fn log_path() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("RFSEE_MEM_LOG") {
        path.into()
    } else {
        concat!(env!("CARGO_MANIFEST_DIR"), "/memory-profile.csv").into()
    }
}

fn append_log(record: &LogRecord) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;

    let path = log_path();
    let needs_header = !path.exists();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    if needs_header {
        writeln!(file, "{LOG_HEADER}")?;
    }
    writeln!(
        file,
        "{},{},{},{},{},{},{},{}",
        record.timestamp,
        record.git_sha,
        record.mode,
        record.docs,
        record.index_terms,
        record.alloc_count,
        record.peak_heap_bytes,
        record.peak_rss_kib,
    )?;
    Ok(path)
}

fn unix_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Short SHA of HEAD, with a `-dirty` suffix when the working tree has uncommitted
/// changes so profiling runs mid-fix aren't mistaken for clean-commit numbers.
fn git_sha() -> String {
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false);

    if dirty { format!("{sha}-dirty") } else { sha }
}
