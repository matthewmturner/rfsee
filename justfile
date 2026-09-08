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

# Build the real index and print time, memory, CPU, I/O, and other resource stats.
# Pass verbosity without a dash, for example: just profile-build-index vv
profile-build-index v="":
    #!/bin/sh
    # macOS `time -l`: https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man1/time.1.html
    # GNU `time -v`: https://www.gnu.org/software/time/manual/html_node/Invoking-time.html
    set -eu
    cargo build --release --package rfsee
    case "$(uname -s)" in
    Darwin) exec /usr/bin/time -l -h target/release/rfsee index {{ if v == "" { "" } else { "-" + v } }} ;;
    Linux) exec /usr/bin/time -v target/release/rfsee index {{ if v == "" { "" } else { "-" + v } }} ;;
    *) echo "profile-build-index supports macOS and Linux" >&2; exit 1 ;;
    esac

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
    cargo run --release -p benches --bin perf --features linux-perf

# Profile pipeline memory using deterministic synthetic data (default) or the actual
# downloaded RFC corpus. Appends run and phase measurements to memory-profile.csv.
memory-profile profile="synthetic":
    cargo run --release -p benches --bin benches -- {{profile}}
