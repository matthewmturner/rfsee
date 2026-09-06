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

# Profile pipeline memory: alloc count, peak heap bytes, peak RSS (VmHWM)
memory-profile mode="buffered":
    cargo run --release -p benches -- {{mode}}
