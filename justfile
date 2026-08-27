# Build the debug executable used by local examples.
build:
    cargo build --manifest-path src-tauri/Cargo.toml

# Build the release executable.
build-release:
    cargo build --release --manifest-path src-tauri/Cargo.toml

# Build and run a debug executable with a protocol fixture.
test file="dialog-confirm.json":
    cargo run --manifest-path src-tauri/Cargo.toml -- request protocol/examples/{{file}}

# Build the debug executable, then run a protocol fixture.
build-test file="dialog-confirm.json": build
    cargo run --manifest-path src-tauri/Cargo.toml -- request protocol/examples/{{file}}

# Build the release executable, then run a protocol fixture.
build-test-release file="dialog-confirm.json": build-release
    cargo run --release --manifest-path src-tauri/Cargo.toml -- request protocol/examples/{{file}}
