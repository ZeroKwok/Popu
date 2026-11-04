"""Build the portable Windows wheel from prepared UI assets and icons."""
from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys

from native_payload import WINDOWS_TARGET
from project_paths import ROOT, native_executable, require_windows


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--outdir", type=Path, default=ROOT / "dist")
    args = parser.parse_args()
    require_windows("Portable wheel builds")
    if not shutil.which("cargo-about"):
        raise SystemExit("Install the license tool first: cargo install cargo-about --locked --version 0.9.3 --features cli")
    # An explicit target keeps the host's procedural macros dynamically linked.
    environment = os.environ.copy()
    flags = environment.get("CARGO_ENCODED_RUSTFLAGS")
    if flags is None:
        flags = "\x1f".join(environment.get("RUSTFLAGS", "").split())
    environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(filter(None, [flags, "-C", "target-feature=+crt-static"])) # 静态运行时
    subprocess.run(["cargo", "build", "--locked", "--release", "--target", WINDOWS_TARGET,
                    "--manifest-path", str(ROOT / "src-tauri/Cargo.toml")], cwd=ROOT, env=environment, check=True)
    notices = ROOT / "python/build/RUST_THIRD_PARTY_LICENSES.txt"
    notices.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["cargo", "about", "generate", "--locked", "--fail", "--manifest-path", str(ROOT / "src-tauri/Cargo.toml"),
                    "--config", str(ROOT / "src-tauri/about.toml"), "--output-file", str(notices), str(ROOT / "src-tauri/about.hbs")],
                   cwd=ROOT, check=True)
    lock_hash = hashlib.sha256((ROOT / "src-tauri/Cargo.lock").read_bytes()).hexdigest()
    text = notices.read_text(encoding="utf-8")
    notices.write_text(f"Cargo.lock SHA-256: {lock_hash}\n\n{text}", encoding="utf-8", newline="\n")
    subprocess.run([sys.executable, str(ROOT / "scripts/stage-python-runtime.py"), "--executable",
                    str(native_executable("release", target=WINDOWS_TARGET))], cwd=ROOT, check=True)
    subprocess.run([sys.executable, "-m", "build", "--wheel", "--outdir", str(args.outdir.resolve()), str(ROOT / "python")], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
