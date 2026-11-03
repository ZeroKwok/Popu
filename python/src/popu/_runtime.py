"""Resolve bundled native tools into immutable, per-user runtime directories."""
from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import sys
import tempfile
from pathlib import Path

from ._errors import PopuError
from ._version import __version__


def _digest(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _bundle_directory() -> Path:
    return Path(__file__).resolve().parent / "_native"


def _cache_directory() -> Path:
    local = os.environ.get("LOCALAPPDATA")
    return (Path(local) if local else Path.home() / "AppData" / "Local") / "Popu" / "runtime"


def _manifest(bundle: Path) -> tuple[dict, bytes]:
    try:
        raw = (bundle / "manifest.json").read_bytes()
        value = json.loads(raw)
    except (OSError, ValueError) as error:
        raise PopuError("Bundled Popu runtime is missing or invalid. Reinstall the Windows wheel; for source development, pass Client(executable=...).") from error
    if not isinstance(value, dict) or value.get("version") != __version__ or value.get("platform") != "win_amd64":
        raise PopuError("Bundled Popu runtime does not match this SDK version/platform.")
    files = value.get("files")
    if not isinstance(files, dict) or "popu.exe" not in files:
        raise PopuError("Bundled Popu manifest must identify popu.exe.")
    for name, digest in files.items():
        if not isinstance(name, str) or name in (".", "..") or not re.fullmatch(r"[A-Za-z0-9_.-]+", name):
            raise PopuError("Bundled Popu manifest contains an invalid filename.")
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise PopuError("Bundled Popu manifest contains an invalid SHA-256 digest.")
    return files, raw


def _verify(directory: Path, files: dict) -> None:
    for name, expected in files.items():
        if _digest(directory / name) != expected:
            raise PopuError(f"Popu runtime checksum mismatch: {directory / name}")


def executable_path() -> Path:
    """Use this SDK's payload, never a potentially unrelated popu on PATH."""
    if sys.platform != "win32" or sys.maxsize <= 2**32:
        raise PopuError("The bundled Popu runtime supports 64-bit Python on Windows only.")
    bundle = _bundle_directory()
    files, raw = _manifest(bundle)
    identity = hashlib.sha256(raw).hexdigest()[:16]
    root = _cache_directory().absolute()
    destination = root / f"{__version__}-win_amd64-{identity}"
    staging: Path | None = None
    try:
        # Resolve only after creation: Windows can otherwise return different
        # extended-path spellings when concurrent callers create the parent.
        root.mkdir(parents=True, exist_ok=True)
        root = root.resolve()
        destination = root / destination.name
        if destination.exists():
            # Never replace a potentially running executable in place.
            _verify(destination, files)
            return destination / "popu.exe"
        staging = Path(tempfile.mkdtemp(prefix=".popu-", dir=root)).resolve()
        for name in files:
            shutil.copyfile(bundle / name, staging / name)
        _verify(staging, files)
        (staging / "manifest.json").write_bytes(raw)
        try:
            staging.rename(destination)
        except OSError:
            if not destination.is_dir():
                raise
            # Another caller may have published the same complete runtime.
            _verify(destination, files)
        return destination / "popu.exe"
    except OSError as error:
        raise PopuError(f"Could not prepare Popu runtime at {destination}: {error}") from error
    finally:
        if staging is not None and staging.exists():
            # Only remove our own unique staging directory under this root.
            if staging.parent.resolve() == root:
                shutil.rmtree(staging)
