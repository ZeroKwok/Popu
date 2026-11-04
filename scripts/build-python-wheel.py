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

LICENSE_TOOL_VERSION = "0.9.3"


def license_tool() -> Path:
    """Reuse the exact tool version, or install it in the project's build cache."""
    name = "cargo-about.exe" if os.name == "nt" else "cargo-about"
    directory = ROOT / "python/build/tools" / f"cargo-about-{LICENSE_TOOL_VERSION}"
    cached = directory / "bin" / name
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    candidates = [shutil.which("cargo-about"), cached, cargo_home / "bin" / name]

    def matches(path: Path) -> bool:
        if not path.is_file():
            return False
        try:
            result = subprocess.run([str(path), "--version"], capture_output=True, text=True, timeout=10)
        except (OSError, subprocess.TimeoutExpired):
            return False
        return result.returncode == 0 and result.stdout.strip() == f"cargo-about {LICENSE_TOOL_VERSION}"

    for candidate in candidates:
        if candidate and matches(Path(candidate)):
            return Path(candidate).resolve()
    print(f"Preparing cargo-about {LICENSE_TOOL_VERSION} in {directory}. First-time compilation may take a few minutes.", flush=True)
    subprocess.run(["cargo", "install", "cargo-about", "--locked", "--version", LICENSE_TOOL_VERSION,
                    "--features", "cli", "--root", str(directory)], cwd=ROOT, check=True)
    if not matches(cached):
        raise RuntimeError(f"License tool installation did not provide cargo-about {LICENSE_TOOL_VERSION}: {cached}")
    return cached.resolve()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--outdir", type=Path, default=ROOT / "dist")
    args = parser.parse_args()
    require_windows("Portable wheel builds")
    about = license_tool()
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
    subprocess.run([str(about), "generate", "--locked", "--fail", "--manifest-path", str(ROOT / "src-tauri/Cargo.toml"),
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
