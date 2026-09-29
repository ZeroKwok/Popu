# Build the debug executable used by local examples.
build:
    cargo build --manifest-path src-tauri/Cargo.toml

# Regenerate committed application and tray icons from the SVG source.
icons:
    python scripts/generate-icons.py

# Build the release executable.
build-release:
    cargo build --release --manifest-path src-tauri/Cargo.toml

# Clean the build artifacts for the debug and release executables.
clean:
    cargo clean --manifest-path src-tauri/Cargo.toml

# Run a fixture with its optional trusted external-template sidecar.
cli executable request:
    #!/usr/bin/env bash
    set -euo pipefail
    executable="{{executable}}"
    request="{{request}}"
    template_sidecar="${request%.json}.template"
    args=(request "$request")
    
    if [[ -f "$template_sidecar" ]]; then
        IFS= read -r template_dir < "$template_sidecar"
        template_dir="${template_dir%$'\r'}"
        if [[ -z "$template_dir" ]]; then
            echo "Empty template sidecar: $template_sidecar" >&2
            exit 12
        fi
        args+=(--template "$template_dir")
    fi
    "$executable" "${args[@]}"

# Build and run a debug executable with a protocol fixture.
test file="dialog-confirm.json": build
    just --quiet cli "src-tauri/target/debug/popu" "protocol/examples/{{file}}"

# Build once, then run protocol fixtures whose filenames begin with `prefix`.
# For example: `just build-test notification` or `just build-test dialog`.
build-test prefix="": build
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    prefix="{{prefix}}"
    examples=(protocol/examples/"$prefix"*.json)
    executable=src-tauri/target/debug/popu
    
    if (( ${#examples[@]} == 0 )); then
        echo "No protocol examples found for prefix: $prefix" >&2
        exit 1
    fi
    for example in "${examples[@]}"; do
        printf '\n==> %s\n' "$example"
        if just --quiet cli "$executable" "$example"; then
            continue
        else
            status=$?
        fi
        case "$status" in
            10|11) ;;
            *) exit "$status" ;;
        esac
    done

# Build the release executable, then run a protocol fixture.
build-test-release file="dialog-confirm.json": build-release
    just --quiet cli "src-tauri/target/release/popu" "protocol/examples/{{file}}"

# Build the release executable, then run a protocol fixture.
install: build-release
    #!/usr/bin/env bash
    cp -v ./src-tauri/target/release/popu.exe /g/Local/bin/

# Run every protocol fixture through the shared prefix-based entry point.
build-test-all:
    just build-test
