publish-dry-run:
    cargo publish -n --manifest-path crates/tf_idf/Cargo.toml
    cargo publish -n --manifest-path crates/cli/Cargo.toml

generate-test-index:
    cargo r --manifest-path tests/generate-data/Cargo.toml

build:
    cargo build --release

build-dev:
    cargo build

time-build-index:
    time cargo r --release --package rfsee -- index

bench:
    cargo bench -p benches

bench-baseline name="before":
    cargo bench -p benches -- --save-baseline {{name}}

bench-compare name="before":
    cargo bench -p benches -- --baseline {{name}}

# Hardware-counter snapshot of the search path (Linux perf_event_open): cycles,
# instructions, cache refs/misses, branch misses, plus IPC and miss rates.
# Configure with RFSEE_BENCH_DOCS / RFSEE_PERF_ITERS; appends to benches/perf-profile.csv.
bench-perf:
    cargo run --release -p benches --bin perf

# Profile pipeline memory: alloc count, peak heap bytes, peak RSS (VmHWM)
memory-profile mode="buffered":
    cargo run --release -p benches -- {{mode}}
