//! Memory profiler for the index-building pipeline.
//!
//! This is not a criterion benchmark: allocation counting requires a process-wide
//! `#[global_allocator]`, so it runs as a standalone binary:
//!
//! ```sh
//! cargo run --release -p benches --bin benches -- synthetic
//! cargo run --release -p benches --bin benches -- actual
//! RFSEE_BENCH_DOCS=2000 cargo run --release -p benches --bin benches -- synthetic
//! ```
//!
//! `synthetic` buffers the deterministic generated corpus before processing it.
//! `actual` downloads and indexes the real RFC corpus through the production
//! streaming `par_load_rfcs_with_report` path.
//!
//! Reports allocation count and peak live heap bytes (via the counting allocator)
//! plus total process peak RSS (VmHWM from /proc/self/status). It also attributes
//! allocations, retained heap, and temporary peak growth to the input, ingest, and
//! finish phases in `memory-profile.csv`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use benches::{corpus, doc_count_from_env};
use rfsee_tf_idf::{Runtime, TfIdf};

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

    fn snapshot(&self) -> AllocSnapshot {
        AllocSnapshot {
            allocs: self.allocs.load(Ordering::Relaxed),
            live_bytes: self.live_bytes.load(Ordering::Relaxed),
        }
    }

    /// Begin a new high-water measurement while preserving currently live memory.
    fn start_phase(&self) -> AllocSnapshot {
        let snapshot = self.snapshot();
        self.peak_bytes
            .store(snapshot.live_bytes, Ordering::Relaxed);
        snapshot
    }
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            self.allocs.fetch_add(1, Ordering::Relaxed);
            let live = self.live_bytes.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
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

#[derive(Clone, Copy)]
struct AllocSnapshot {
    allocs: usize,
    live_bytes: usize,
}

#[derive(Clone, Copy)]
struct PhaseMetrics {
    name: &'static str,
    start_elapsed_ms: f64,
    end_elapsed_ms: f64,
    duration_ms: f64,
    alloc_count: usize,
    retained_bytes: i64,
    peak_growth_bytes: usize,
    heap_start_bytes: usize,
    heap_end_bytes: usize,
    heap_peak_bytes: usize,
}

fn measure_phase<T>(
    name: &'static str,
    run_start: std::time::Instant,
    work: impl FnOnce() -> T,
) -> (T, PhaseMetrics) {
    let start = ALLOC.start_phase();
    let start_elapsed_ms = elapsed_ms(run_start);
    let value = work();
    let end_elapsed_ms = elapsed_ms(run_start);
    let end = ALLOC.snapshot();
    let peak = ALLOC.peak_bytes.load(Ordering::Relaxed);
    (
        value,
        PhaseMetrics {
            name,
            start_elapsed_ms,
            end_elapsed_ms,
            duration_ms: end_elapsed_ms - start_elapsed_ms,
            alloc_count: end.allocs - start.allocs,
            retained_bytes: signed_delta(end.live_bytes, start.live_bytes),
            peak_growth_bytes: peak.saturating_sub(start.live_bytes),
            heap_start_bytes: start.live_bytes,
            heap_end_bytes: end.live_bytes,
            heap_peak_bytes: peak,
        },
    )
}

fn elapsed_ms(start: std::time::Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1_000.0
}

fn signed_delta(end: usize, start: usize) -> i64 {
    let magnitude = end.abs_diff(start).min(i64::MAX as usize) as i64;
    if end >= start {
        magnitude
    } else {
        -magnitude
    }
}

/// Peak RSS (VmHWM) of this process in KiB.
#[cfg(target_os = "linux")]
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

#[cfg(target_os = "macos")]
fn peak_rss_kib() -> u64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // Darwin reports ru_maxrss in bytes; normalize it to the Linux/CSV KiB unit:
    // https://github.com/apple/darwin-xnu/blob/main/bsd/man/man2/getrusage.2
    let succeeded = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0;
    if succeeded {
        unsafe { usage.assume_init() }
            .ru_maxrss
            .try_into()
            .map(|bytes: u64| bytes / 1024)
            .unwrap_or(0)
    } else {
        0
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn peak_rss_kib() -> u64 {
    0
}

extern "C" fn noop_cb(_: *const std::ffi::c_char) {}

fn main() {
    let profile = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "synthetic".to_string());
    if profile != "synthetic" && profile != "actual" {
        eprintln!("unknown profile '{profile}', expected 'synthetic' or 'actual'");
        std::process::exit(2);
    }

    let run_start = std::time::Instant::now();
    let workload_start = ALLOC.snapshot();
    let rss_start_kib = peak_rss_kib();

    // Keep the production runtime alive through the final measurements in actual mode.
    let (index, docs, phases, _runtime) = if profile == "synthetic" {
        let docs = doc_count_from_env();
        let (entries, input) = measure_phase("input", run_start, || corpus(docs));
        let (mut index, ingest) = measure_phase("ingest", run_start, || {
            let mut index = TfIdf::default();
            for entry in entries {
                index.add_rfc_entry(entry);
            }
            index
        });
        let ((), finish) = measure_phase("finish", run_start, || index.finish(noop_cb));
        (index, docs, vec![input, ingest, finish], None)
    } else {
        let (loaded, load_and_ingest) = measure_phase("load_and_ingest", run_start, || {
            let runtime = Runtime::default();
            let mut index = TfIdf::default();
            let report = index.par_load_rfcs_with_report(&runtime, noop_cb);
            (index, runtime, report)
        });
        let (mut index, runtime, report) = match loaded {
            (index, runtime, Ok(report)) => (index, runtime, report),
            (_, _, Err(error)) => {
                eprintln!("actual memory profile failed while loading RFCs: {error}");
                std::process::exit(1);
            }
        };
        let docs = report.loaded.len();
        eprintln!(
            "actual RFCs: {} loaded, {} skipped, {} total",
            docs,
            report.failures.len(),
            report.total
        );
        let ((), finish) = measure_phase("finish", run_start, || index.finish(noop_cb));
        (index, docs, vec![load_and_ingest, finish], Some(runtime))
    };

    // Keep the index alive until after the final measurements.
    let terms = std::hint::black_box(&index.index.term_scores).len();

    let workload_end = ALLOC.snapshot();
    let allocs = workload_end.allocs - workload_start.allocs;
    let peak_heap = phases
        .iter()
        .map(|phase| phase.heap_peak_bytes)
        .max()
        .unwrap_or(workload_start.live_bytes);
    let rss_end_kib = peak_rss_kib();

    println!("profile={profile}");
    println!("docs={docs}");
    println!("index_terms={terms}");
    println!("alloc_count={allocs}");
    println!("peak_heap_bytes={peak_heap}");
    println!("peak_rss_kib_start={rss_start_kib}");
    println!("peak_rss_kib_end={rss_end_kib}");
    for phase in &phases {
        println!(
            "phase={} start_elapsed_ms={:.3} end_elapsed_ms={:.3} duration_ms={:.3} alloc_count={} retained_bytes={} peak_growth_bytes={} heap_end_bytes={}",
            phase.name,
            phase.start_elapsed_ms,
            phase.end_elapsed_ms,
            phase.duration_ms,
            phase.alloc_count,
            phase.retained_bytes,
            phase.peak_growth_bytes,
            phase.heap_end_bytes,
        );
    }

    let record = LogRecord {
        timestamp: unix_timestamp_ms(),
        git_sha: git_sha(),
        profile: profile.clone(),
        docs,
        index_terms: terms,
        peak_heap_bytes: peak_heap,
        peak_rss_kib: rss_end_kib,
    };
    match append_log(&record, &phases) {
        Ok(path) => println!("logged_to={}", path.display()),
        Err(e) => eprintln!("failed to write memory profile log: {e}"),
    }
}

struct LogRecord {
    timestamp: u64,
    git_sha: String,
    profile: String,
    docs: usize,
    index_terms: usize,
    peak_heap_bytes: usize,
    peak_rss_kib: u64,
}

const LOG_HEADER: &str = "timestamp_ms,git_sha,profile,docs,index_terms,phase,start_elapsed_ms,end_elapsed_ms,duration_ms,alloc_count,retained_bytes,peak_growth_bytes,heap_start_bytes,heap_end_bytes,heap_peak_bytes,run_peak_heap_bytes,peak_rss_kib";

/// Default log path, anchored to this crate so cwd doesn't matter. Override with
/// the RFSEE_MEM_LOG env var.
fn log_path() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("RFSEE_MEM_LOG") {
        path.into()
    } else {
        concat!(env!("CARGO_MANIFEST_DIR"), "/memory-profile.csv").into()
    }
}

fn append_log(record: &LogRecord, phases: &[PhaseMetrics]) -> std::io::Result<std::path::PathBuf> {
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
    for phase in phases {
        writeln!(
            file,
            "{},{},{},{},{},{},{:.3},{:.3},{:.3},{},{},{},{},{},{},{},{}",
            record.timestamp,
            record.git_sha,
            record.profile,
            record.docs,
            record.index_terms,
            phase.name,
            phase.start_elapsed_ms,
            phase.end_elapsed_ms,
            phase.duration_ms,
            phase.alloc_count,
            phase.retained_bytes,
            phase.peak_growth_bytes,
            phase.heap_start_bytes,
            phase.heap_end_bytes,
            phase.heap_peak_bytes,
            record.peak_heap_bytes,
            record.peak_rss_kib,
        )?;
    }
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

    if dirty {
        format!("{sha}-dirty")
    } else {
        sha
    }
}

#[cfg(test)]
mod tests {
    use super::signed_delta;

    #[test]
    fn signed_delta_reports_growth_and_release() {
        assert_eq!(signed_delta(15, 10), 5);
        assert_eq!(signed_delta(10, 15), -5);
        assert_eq!(signed_delta(10, 10), 0);
    }
}
