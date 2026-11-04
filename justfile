python := env("PYTHON", "python")

# Show available tasks without installing dependencies or building.
[group("Project")]
default:
    @just --list

# Install UI dependencies and prepare generated source-build inputs.
[group("Project")]
setup: setup-ui prep-python prep-native

# Print the single product version.
[group("Project")]
version:
    @"{{ python }}" python/build_version.py --print

# Test the cross-project development and build helpers.
[group("Project")]
test-tools:
    "{{ python }}" -m pytest scripts/tests -q

# Run static UI checks and automated UI, tool, SDK, and Rust tests.
[group("Project")]
check: check-ui test-ui test-tools test-python test-rust

# Install dependencies of the UI npm project.
[group("UI")]
[working-directory("ui")]
setup-ui:
    npm ci

# Build the static UI distribution consumed by Cargo.
[group("UI")]
[working-directory("ui")]
build-ui:
    npm run build

# Check UI and chart-entry JavaScript syntax.
[group("UI")]
[working-directory("ui")]
check-ui:
    npm run check

# Run UI unit tests without opening native windows.
[group("UI")]
[working-directory("ui")]
test-ui:
    npm test

# Build the UI and run browser visual/interaction regression tests.
[group("UI")]
[working-directory("ui")]
test-visuals:
    npm run test:visuals

# Generate missing or outdated native icons before compiling.
[group("Rust")]
prep-native:
    "{{ python }}" scripts/generate-icons.py --if-needed

# Force regeneration of application and tray icons from SVG.
[group("Rust")]
icons:
    "{{ python }}" scripts/generate-icons.py

# Build the debug CLI/Host after preparing UI assets and native icons.
[group("Rust")]
build: build-ui prep-native
    cargo build --manifest-path src-tauri/Cargo.toml

# Build the release CLI/Host after preparing UI assets and native icons.
[group("Rust")]
build-release: build-ui prep-native
    cargo build --release --manifest-path src-tauri/Cargo.toml

# Run Rust unit/integration tests; interactive window tests remain opt-in.
[group("Rust")]
test-rust: build-ui prep-native
    cargo test --locked --manifest-path src-tauri/Cargo.toml

# Check Rust formatting without changing source files.
[group("Rust")]
fmt-check:
    cargo fmt --manifest-path src-tauri/Cargo.toml -- --check

# Windows/WebView2 smoke test; exit the existing Host before running.
[group("Rust")]
[windows]
test-native: build
    "{{ python }}" scripts/dev.py native-smoke

# Clean Cargo build artifacts only (respects CARGO_TARGET_DIR).
[group("Rust")]
clean:
    cargo clean --manifest-path src-tauri/Cargo.toml

# Generate the SDK version from Cargo (also automatic during pip installation).
[group("Python")]
prep-python:
    "{{ python }}" python/build_version.py

# Run SDK tests after preparing its generated version module.
[group("Python")]
test-python: prep-python
    "{{ python }}" -m pytest python/tests -q

# Build a local Windows wheel; publishing remains a separate release step.
[group("Python")]
[windows]
build-wheel: build-ui prep-native
    "{{ python }}" scripts/build-python-wheel.py

# Run the packaged progress demo with an installed SDK and a local debug build.
[group("Demo")]
[positional-arguments]
demo-python *args: prep-python
    "{{ python }}" scripts/dev.py python -- "$@"

# Build and run one JSON fixture and its optional trusted template sidecar.
[group("Demo")]
[positional-arguments]
demo-fixture file="dialog-confirm.json": build
    "{{ python }}" scripts/dev.py fixture -- "$1"

# Build once, then run fixtures with a filename prefix (empty means all).
[group("Demo")]
[positional-arguments]
demo-fixtures prefix="": build
    "{{ python }}" scripts/dev.py fixtures -- "$1"

# Build the release executable, then run a protocol fixture.
[group("Demo")]
[positional-arguments]
demo-fixture-release file="dialog-confirm.json": build-release
    "{{ python }}" scripts/dev.py fixture --profile release -- "$1"
