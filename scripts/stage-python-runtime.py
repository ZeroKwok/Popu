"""Stage a Windows x64 runtime for release packaging or editable development.

Requires Python 3.11+ for TOML parsing; installed SDKs support Python 3.10+.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from project_paths import native_executable, require_windows
from native_payload import WINDOWS_TARGET, imported_dlls, verify_portable_binary

ROOT = Path(__file__).resolve().parents[1]

sys.path.insert(0, str(ROOT / "python"))
from build_version import generate_version


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dev", action="store_true", help="Stage a development payload from the debug build, without release license or static CRT checks")
    parser.add_argument("--executable", type=Path, help="Override the binary path; defaults to debug with --dev, otherwise the wheel target's release directory")
    parser.add_argument("--dll", type=Path, action="append", default=[], help="Additional native runtime DLL; repeat if needed")
    args = parser.parse_args(argv)
    require_windows("Native runtime staging")
    version = generate_version()
    executable = args.executable or (native_executable() if args.dev else native_executable("release", target=WINDOWS_TARGET))
    executable = executable.resolve(strict=True)
    verify_binary = imported_dlls if args.dev else verify_portable_binary
    verify_binary(executable.read_bytes())
    result = subprocess.run([str(executable), "--version"], capture_output=True, text=True, encoding="utf-8", check=True, timeout=10)
    if result.stdout.strip() != f"popu {version}":
        raise ValueError(f"executable version mismatch: {result.stdout.strip()}")
    sources = {"popu.exe": executable}
    if not args.dev:
        sources.update({
            "LICENSE": ROOT / "LICENSE",
            "THIRD_PARTY_LICENSES.txt": ROOT / "ui/dist/vendor/THIRD_PARTY_LICENSES.txt",
            "RUST_THIRD_PARTY_LICENSES.txt": ROOT / "python/build/RUST_THIRD_PARTY_LICENSES.txt",
        })
        python_license = (ROOT / "python/LICENSE").read_bytes().replace(b"\r\n", b"\n")
        project_license = sources["LICENSE"].read_bytes().replace(b"\r\n", b"\n")
        if python_license != project_license:
            raise ValueError("python/LICENSE does not match the repository LICENSE")
        lock_hash = hashlib.sha256((ROOT / "src-tauri/Cargo.lock").read_bytes()).hexdigest()
        if not sources["RUST_THIRD_PARTY_LICENSES.txt"].read_text(encoding="utf-8").startswith(f"Cargo.lock SHA-256: {lock_hash}\n"):
            raise ValueError("Native license notices are stale; rebuild with `just build-wheel`.")
    for library in args.dll:
        if not re.fullmatch(r"[A-Za-z0-9_.-]+", library.name) or library.suffix.lower() != ".dll" or library.name.lower() in {name.lower() for name in sources}:
            raise ValueError(f"invalid or duplicate DLL: {library}")
        verify_binary(library.read_bytes())
        sources[library.name] = library.resolve(strict=True)
    destination = ROOT / "python/src/popu/_native"
    with tempfile.TemporaryDirectory(prefix="popu-stage-", dir=ROOT / "python") as temporary:
        staging = Path(temporary)
        hashes = {}
        for name, source in sources.items():
            shutil.copyfile(source, staging / name)
            hashes[name] = hashlib.sha256((staging / name).read_bytes()).hexdigest()
        manifest = {"version": version, "platform": "win_amd64", "files": hashes}
        if args.dev:
            manifest["development"] = True
        (staging / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        # This exact generated payload directory is never an execution location.
        if destination.resolve().parent != (ROOT / "python/src/popu").resolve():
            raise ValueError("payload directory is outside the Python package")
        if destination.exists():
            shutil.rmtree(destination)
        shutil.copytree(staging, destination)
    mode = "development" if args.dev else "release"
    print(f"Staged {mode} popu {version} ({hashes['popu.exe'][:12]}) at {destination}")


if __name__ == "__main__":
    main()
