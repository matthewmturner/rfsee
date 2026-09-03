build-artifacts:
    cargo build --release --manifest-path crates/ffi/Cargo.toml

[macos]
copy-artifacts:
    cp target/release/libffi.dylib artifacts/

[linux]
copy-artifacts:
    cp target/release/libffi.so artifacts/

generate-artifacts: build-artifacts copy-artifacts

publish-dry-run:
    cargo publish -n --manifest-path crates/tf_idf/Cargo.toml
    cargo publish -n --manifest-path crates/cli/Cargo.toml

generate-test-index:
    cargo r --manifest-path tests/generate-data/Cargo.toml

lua-integration-test:
    luajit tests/search_index.lua

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
