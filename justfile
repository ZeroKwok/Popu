# Build the debug executable used by local examples.
build:
    cargo build --manifest-path src-tauri/Cargo.toml

# Build the release executable.
build-release:
    cargo build --release --manifest-path src-tauri/Cargo.toml

# Clean the build artifacts for the debug and release executables.
clean:
    cargo clean --manifest-path src-tauri/Cargo.toml

# Build and run a debug executable with a protocol fixture.
test file="dialog-confirm.json":
    cargo run --manifest-path src-tauri/Cargo.toml -- request protocol/examples/{{file}}

# Build the debug executable, then run a protocol fixture.
build-test file="dialog-confirm.json": build
    cargo run --manifest-path src-tauri/Cargo.toml -- request protocol/examples/{{file}}

# Build the release executable, then run a protocol fixture.
build-test-release file="dialog-confirm.json": build-release
    cargo run --release --manifest-path src-tauri/Cargo.toml -- request protocol/examples/{{file}}

# Build once, then run every protocol fixture in filename order. Each request
# waits for Popu to exit before the next fixture starts.
build-test-all: build
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    examples=(protocol/examples/*.json)
    executable=src-tauri/target/debug/popu
    
    if (( ${#examples[@]} == 0 )); then
        echo "No protocol examples found in protocol/examples" >&2
        exit 1
    fi
    for example in "${examples[@]}"; do
        printf '\n==> %s\n' "$example"
        "$executable" request "$example" || echo ''
    done
