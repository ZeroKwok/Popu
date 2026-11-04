"""Shared repository and Cargo artifact paths for development commands."""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def native_executable(profile: str = "debug", *, target: str | None = None) -> Path:
    if profile not in ("debug", "release"):
        raise ValueError(f"Unsupported build profile: {profile}")
    # Let Cargo resolve CARGO_TARGET_DIR and .cargo/config.toml.
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--manifest-path", str(ROOT / "src-tauri/Cargo.toml")],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8", check=True,
    )
    directory = Path(json.loads(result.stdout)["target_directory"])
    if target is not None:
        directory /= target
    return directory / profile / ("popu.exe" if os.name == "nt" else "popu")


def require_executable(path: Path) -> Path:
    if not path.is_file():
        raise FileNotFoundError(f"Native executable not found: {path}. Run `just build` or `just build-release` first.")
    return path


def require_windows(feature: str) -> None:
    if sys.platform != "win32":
        raise RuntimeError(f"{feature} currently supports Windows only.")
