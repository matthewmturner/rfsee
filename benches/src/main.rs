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
}
