//! Hardware-counter snapshot of the search path (Linux `perf_event_open`).
//!
//! This is not a criterion benchmark: the point is capturing every counter in one
//! invocation so derived ratios (IPC, cache-miss rate, branch-miss rate) come from
//! the same corpus and machine state. Criterion's `Measurement` trait reports one
//! scalar per sample, which forces one run per event.
//!
//! ```sh
//! cargo run --release -p benches --bin perf --features linux-perf
//! RFSEE_PERF_ITERS=200 cargo run --release -p benches --bin perf --features linux-perf
//! ```
//!
//! Loads the dataset written by `just generate-bench-data`; select a size with
//! RFSEE_BENCH_SIZE (default 1000).
//!
//! Workloads: warm search (algorithm only, in-memory index) for common/rare/multi
//! queries, cold search (read + parse + search, what the CLI pays per query), and
//! `combine_scores` in isolation.
//!
//! Counter groups are scheduled all-or-nothing, and PMUs offer few programmable
//! counters (4-6 per core, minus one when the NMI watchdog is on), so the six
//! events are measured in two passes of four: {cycles, instructions, cache refs,
//! cache misses} then {cycles, instructions, branch insns, branch misses}. If the
//! kernel still has to time-multiplex a pass, its results are flagged as estimates.
//!
//! The counters exclude kernel and hypervisor events (the perf-event crate default),
//! so this works with `kernel.perf_event_paranoid <= 2`. Iteration counts are
//! calibrated to ~250ms of measured work per pass; set RFSEE_PERF_ITERS for a fixed
//! count. Results are appended to perf-profile.csv (override with RFSEE_PERF_LOG),
//! mirroring the memory profiler.

use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

use perf_event::events::Hardware;
use perf_event::{Builder, Counter, Group};

const COMMON_QUERY: &str = "term0";
const RARE_QUERY: &str = "term19999";
const MULTI_QUERY: &str = "term0 term1 term2 term3 term4";

/// Target measured time per pass; iteration count is calibrated to hit this.
const TARGET_MEASURED: Duration = Duration::from_millis(250);

/// First pass: cycles/instructions plus the cache pair (miss rate needs both).
const CORE_EVENTS: &[Hardware] = &[
    Hardware::CPU_CYCLES,
    Hardware::INSTRUCTIONS,
    Hardware::CACHE_REFERENCES,
    Hardware::CACHE_MISSES,
];
/// Second pass: cycles/instructions repeated (IPC cross-check) plus branch pair.
const BRANCH_EVENTS: &[Hardware] = &[
    Hardware::CPU_CYCLES,
    Hardware::INSTRUCTIONS,
    Hardware::BRANCH_INSTRUCTIONS,
    Hardware::BRANCH_MISSES,
];

/// A perf counter group for one pass. Kept small enough to be schedulable as a
/// unit on width-4 PMUs; a group that doesn't fit is never scheduled at all.
struct Pass {
    group: Group,
    counters: Vec<Counter>,
}

impl Pass {
    fn new(kinds: &[Hardware]) -> io::Result<Self> {
        let mut group = Group::new()?;
        let counters = kinds
            .iter()
            .map(|&kind| Builder::new().group(&mut group).kind(kind).build())
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self { group, counters })
    }

    /// Read all counters, prorated if the kernel time-multiplexed the group.
    /// Returns per-event values aligned with the `kinds` passed to `new`, plus a
    /// flag set when values are kernel estimates.
    fn read_scaled(&mut self) -> io::Result<(Vec<f64>, bool)> {
        let counts = self.group.read()?;
        let scale = counts.time_enabled() as f64 / counts.time_running().max(1) as f64;
        let values = self
            .counters
            .iter()
            .map(|c| counts[c] as f64 * scale)
            .collect();
        Ok((values, scale > 1.0))
    }
}

/// Measured per-pass totals for one workload.
struct Totals {
    cycles: f64,
    instructions: f64,
    cache_refs: f64,
    cache_misses: f64,
    branch_insns: f64,
    branch_misses: f64,
    /// True when either pass was time-multiplexed and holds kernel estimates.
    multiplexed: bool,
}

/// Run `iters` iterations of `run(setup())` for both passes, counting only `run`.
fn measure<S>(
    iters: usize,
    mut setup: impl FnMut() -> S,
    mut run: impl FnMut(S),
) -> io::Result<Totals> {
    // Warm up before counting.
    for _ in 0..3 {
        run(setup());
    }
    let mut run_pass = |kinds: &[Hardware]| -> io::Result<(Vec<f64>, bool)> {
        let mut pass = Pass::new(kinds)?;
        for _ in 0..iters {
            let input = setup();
            pass.group.enable()?;
            run(input);
            pass.group.disable()?;
        }
        pass.read_scaled()
    };
    let (core, core_mux) = run_pass(CORE_EVENTS)?;
    let (branch, branch_mux) = run_pass(BRANCH_EVENTS)?;
    Ok(Totals {
        cycles: core[0],
        instructions: core[1],
        cache_refs: core[2],
        cache_misses: core[3],
        branch_insns: branch[2],
        branch_misses: branch[3],
        multiplexed: core_mux || branch_mux,
    })
}

/// Pick an iteration count so measured work totals roughly TARGET_MEASURED.
fn calibrate<S>(mut setup: impl FnMut() -> S, mut run: impl FnMut(S)) -> usize {
    if let Ok(v) = std::env::var("RFSEE_PERF_ITERS") {
        match v.parse() {
            Ok(n) => return n,
            Err(_) => eprintln!("invalid RFSEE_PERF_ITERS '{v}', calibrating instead"),
        }
    }
    let start = Instant::now();
    run(setup());
    let elapsed = start.elapsed().max(Duration::from_micros(1));
    (TARGET_MEASURED.as_secs_f64() / elapsed.as_secs_f64()).clamp(5.0, 10_000.0) as usize
}

/// A workload runs once and returns its totals plus the iteration count used.
type Workload<'a> = Box<dyn FnMut() -> (Totals, usize) + 'a>;

fn search_workload<'a>(index: &'a rfsee_tf_idf::Index, query: &'a str) -> Workload<'a> {
    Box::new(move || {
        let setup = || index.clone();
        let run = |index| {
            std::hint::black_box(rfsee_tf_idf::search_index(query.to_string(), index));
        };
        let iters = calibrate(setup, run);
        (
            measure(iters, setup, run).expect("perf counters failed"),
            iters,
        )
    })
}

fn cold_workload(path: &std::path::Path) -> Workload<'_> {
    Box::new(move || {
        let setup = || path.to_path_buf();
        let run = |path: std::path::PathBuf| {
            let file = std::fs::File::open(path).unwrap();
            let index: rfsee_tf_idf::Index = simd_json::from_reader(file).unwrap();
            std::hint::black_box(rfsee_tf_idf::search_index(COMMON_QUERY.to_string(), index));
        };
        let iters = calibrate(setup, run);
        (
            measure(iters, setup, run).expect("perf counters failed"),
            iters,
        )
    })
}

fn combine_workload(maps: &[HashMap<i32, i32>]) -> Workload<'_> {
    Box::new(move || {
        let setup = || maps.to_vec();
        let run = |maps: Vec<HashMap<i32, i32>>| {
            std::hint::black_box(rfsee_tf_idf::combine_scores(maps));
        };
        let iters = calibrate(setup, run);
        (
            measure(iters, setup, run).expect("perf counters failed"),
            iters,
        )
    })
}

fn thousands(n: f64) -> String {
    let s = format!("{}", n.round() as u64);
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn report(name: &str, docs: usize, iters: usize, t: &Totals) {
    let per = |v: f64| thousands(v / iters as f64);
    let ipc = t.instructions / t.cycles.max(1.0);
    let cache_miss_rate = 100.0 * t.cache_misses / t.cache_refs.max(1.0);
    let branch_miss_rate = 100.0 * t.branch_misses / t.branch_insns.max(1.0);
    let est = if t.multiplexed {
        " [estimated: counters time-multiplexed]"
    } else {
        ""
    };

    println!("== {name} (docs={docs}, iters={iters}){est}");
    println!("  cycles            {:>15} /iter", per(t.cycles));
    println!("  instructions      {:>15} /iter", per(t.instructions));
    println!("  cache_references  {:>15} /iter", per(t.cache_refs));
    println!("  cache_misses      {:>15} /iter", per(t.cache_misses));
    println!("  branch_insns      {:>15} /iter", per(t.branch_insns));
    println!("  branch_misses     {:>15} /iter", per(t.branch_misses));
    println!(
        "  ipc={ipc:.2}  cache_miss_rate={cache_miss_rate:.2}%  \
         branch_miss_rate={branch_miss_rate:.2}%"
    );
}

fn log_csv(
    path: &std::path::Path,
    git_sha: &str,
    name: &str,
    docs: usize,
    iters: usize,
    t: &Totals,
) -> io::Result<()> {
    use std::io::Write;

    const HEADER: &str = "timestamp_ms,git_sha,workload,docs,iters,cycles,instructions,\
                          cache_references,cache_misses,branch_instructions,branch_misses";
    let needs_header = !path.exists();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    if needs_header {
        writeln!(file, "{HEADER}")?;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let per = |v: f64| (v / iters as f64).round() as u64;
    writeln!(
        file,
        "{ts},{git_sha},{name},{docs},{iters},{},{},{},{},{},{}",
        per(t.cycles),
        per(t.instructions),
        per(t.cache_refs),
        per(t.cache_misses),
        per(t.branch_insns),
        per(t.branch_misses),
    )
}

/// Short SHA of HEAD, with `-dirty` when the tree has uncommitted changes (same
/// convention as the memory profiler).
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

fn main() -> io::Result<()> {
    let docs = benches::bench_size_from_env();
    eprintln!("loading bench data ({docs} docs)...");
    let index = benches::load_index();
    let index_path = benches::bench_index_path();
    let score_maps = benches::load_score_maps();

    let mut workloads: Vec<(&str, Workload)> = vec![
        ("warm_common", search_workload(&index, COMMON_QUERY)),
        ("warm_rare", search_workload(&index, RARE_QUERY)),
        ("warm_multi", search_workload(&index, MULTI_QUERY)),
        ("cold_common", cold_workload(&index_path)),
        ("combine_scores", combine_workload(&score_maps)),
    ];

    let git_sha = git_sha();
    let log_path: std::path::PathBuf = std::env::var_os("RFSEE_PERF_LOG")
        .map(Into::into)
        .unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/perf-profile.csv").into());

    for (name, workload) in &mut workloads {
        let (totals, iters) = workload();
        report(name, docs, iters, &totals);
        if let Err(e) = log_csv(&log_path, &git_sha, name, docs, iters, &totals) {
            eprintln!("failed to write perf log: {e}");
        }
    }
    println!("logged_to={}", log_path.display());
    Ok(())
}
