publish-dry-run:
    cargo publish -n --manifest-path crates/tf_idf/Cargo.toml
    cargo publish -n --manifest-path crates/cli/Cargo.toml

generate-test-index:
    cargo r --manifest-path tests/generate-data/Cargo.toml

# Generate bench datasets into bench-data/ (gitignored), one directory per size
# (default: 100 1000 10000). All benches and the perf snapshot load them; select
# a size with RFSEE_BENCH_SIZE (default 1000).
generate-bench-data *sizes:
    cargo run --release -p benches --bin generate_data -- {{sizes}}

build:
    cargo build --release

build-dev:
    cargo build

time-build-index:
    time cargo r --release --package rfsee -- index

# --benches scopes to the criterion targets; without it cargo also runs the
# libtest harness, which rejects criterion flags like --save-baseline.
bench:
    cargo bench -p benches --benches

bench-baseline name="before":
    cargo bench -p benches --benches -- --save-baseline {{name}}

bench-compare name="before":
    cargo bench -p benches --benches -- --baseline {{name}}

# Hardware-counter snapshot of the search path (Linux perf_event_open): cycles,
# instructions, cache refs/misses, branch misses, plus IPC and miss rates.
# Requires bench-data/ (just generate-bench-data); set RFSEE_PERF_ITERS to fix the
# iteration count. Appends to benches/perf-profile.csv.
bench-perf:
    cargo run --release -p benches --bin perf

# Profile pipeline memory: alloc count, peak heap bytes, peak RSS (VmHWM)
memory-profile mode="buffered":
    cargo run --release -p benches -- {{mode}}
