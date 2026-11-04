"""Stage a verified Windows x64 release payload before building the Python wheel.

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
from native_payload import WINDOWS_TARGET, verify_portable_binary

ROOT = Path(__file__).resolve().parents[1]

sys.path.insert(0, str(ROOT / "python"))
from build_version import generate_version


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, help="Portable x64 binary; defaults to the wheel target's release directory")
    parser.add_argument("--dll", type=Path, action="append", default=[], help="Additional native runtime DLL; repeat if needed")
    args = parser.parse_args()
    require_windows("Bundled native wheel preparation")
    version = generate_version()
    executable = (args.executable or native_executable("release", target=WINDOWS_TARGET)).resolve(strict=True)
    verify_portable_binary(executable.read_bytes())
    result = subprocess.run([str(executable), "--version"], capture_output=True, text=True, encoding="utf-8", check=True, timeout=10)
    if result.stdout.strip() != f"popu {version}":
        raise ValueError(f"executable version mismatch: {result.stdout.strip()}")
    sources = {
        "popu.exe": executable,
        "LICENSE": ROOT / "LICENSE",
        "THIRD_PARTY_LICENSES.txt": ROOT / "ui/dist/vendor/THIRD_PARTY_LICENSES.txt",
        "RUST_THIRD_PARTY_LICENSES.txt": ROOT / "python/build/RUST_THIRD_PARTY_LICENSES.txt",
    }
    for library in args.dll:
        if not re.fullmatch(r"[A-Za-z0-9_.-]+", library.name) or library.suffix.lower() != ".dll" or library.name.lower() in {name.lower() for name in sources}:
            raise ValueError(f"invalid or duplicate DLL: {library}")
        verify_portable_binary(library.read_bytes())
        sources[library.name] = library.resolve(strict=True)
    if (ROOT / "python/LICENSE").read_bytes() != sources["LICENSE"].read_bytes():
        raise ValueError("python/LICENSE does not match the repository LICENSE")
    lock_hash = hashlib.sha256((ROOT / "src-tauri/Cargo.lock").read_bytes()).hexdigest()
    if not sources["RUST_THIRD_PARTY_LICENSES.txt"].read_text(encoding="utf-8").startswith(f"Cargo.lock SHA-256: {lock_hash}\n"):
        raise ValueError("Native license notices are stale; rebuild with `just build-wheel`.")
    destination = ROOT / "python/src/popu/_native"
    with tempfile.TemporaryDirectory(prefix="popu-stage-", dir=ROOT / "python") as temporary:
        staging = Path(temporary)
        hashes = {}
        for name, source in sources.items():
            shutil.copyfile(source, staging / name)
            hashes[name] = hashlib.sha256((staging / name).read_bytes()).hexdigest()
        manifest = {"version": version, "platform": "win_amd64", "files": hashes}
        (staging / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        # This exact generated payload directory is never an execution location.
        if destination.resolve().parent != (ROOT / "python/src/popu").resolve():
            raise ValueError("payload directory is outside the Python package")
        if destination.exists():
            shutil.rmtree(destination)
        shutil.copytree(staging, destination)
    print(f"Staged popu {version} ({hashes['popu.exe'][:12]}) at {destination}")


if __name__ == "__main__":
    main()
