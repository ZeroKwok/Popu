"""Stage a verified Windows x64 release payload before building the Python wheel.

Requires Python 3.11+ for TOML parsing; installed SDKs support Python 3.10+.
"""
from __future__ import annotations

import argparse
import ast
import hashlib
import json
import re
import shutil
import struct
import subprocess
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def sdk_version() -> str:
    tree = ast.parse((ROOT / "python/src/popu/_version.py").read_text(encoding="utf-8"))
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(isinstance(target, ast.Name) and target.id == "__version__" for target in node.targets):
            return ast.literal_eval(node.value)
    raise ValueError("SDK version is missing")


def require_x64_pe(executable: Path) -> None:
    with executable.open("rb") as stream:
        if stream.read(2) != b"MZ":
            raise ValueError("runtime must be a Windows PE executable")
        stream.seek(0x3C)
        offset = struct.unpack("<I", stream.read(4))[0]
        stream.seek(offset)
        if stream.read(4) != b"PE\0\0" or struct.unpack("<H", stream.read(2))[0] != 0x8664:
            raise ValueError("runtime must target Windows x64 (AMD64)")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, default=ROOT / "src-tauri/target/release/popu.exe")
    parser.add_argument("--dll", type=Path, action="append", default=[], help="Additional native runtime DLL; repeat if needed")
    args = parser.parse_args()
    version = sdk_version()
    cargo = tomllib.loads((ROOT / "src-tauri/Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    tauri = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))["version"]
    npm = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["version"]
    if len({version, cargo, tauri, npm}) != 1:
        raise ValueError(f"version mismatch: SDK={version}, Cargo={cargo}, Tauri={tauri}, npm={npm}")
    executable = args.executable.resolve(strict=True)
    require_x64_pe(executable)
    result = subprocess.run([str(executable), "--version"], capture_output=True, text=True, encoding="utf-8", check=True, timeout=10)
    if result.stdout.strip() != f"popu {version}":
        raise ValueError(f"executable version mismatch: {result.stdout.strip()}")
    sources = {
        "popu.exe": executable,
        "LICENSE": ROOT / "LICENSE",
        "THIRD_PARTY_LICENSES.txt": ROOT / "ui/vendor/THIRD_PARTY_LICENSES.txt",
    }
    for library in args.dll:
        if not re.fullmatch(r"[A-Za-z0-9_.-]+", library.name) or library.suffix.lower() != ".dll" or library.name.lower() in {name.lower() for name in sources}:
            raise ValueError(f"invalid or duplicate DLL: {library}")
        require_x64_pe(library)
        sources[library.name] = library.resolve(strict=True)
    if (ROOT / "python/LICENSE").read_bytes() != sources["LICENSE"].read_bytes():
        raise ValueError("python/LICENSE does not match the repository LICENSE")
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
