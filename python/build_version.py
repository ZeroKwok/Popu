"""Generate the SDK version from the native application's Cargo manifest."""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import tempfile

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.10 build environments
    import tomli as tomllib

ROOT = Path(__file__).resolve().parent.parent


def cargo_version(root: Path = ROOT) -> str:
    manifest = root / "src-tauri" / "Cargo.toml"
    try:
        with manifest.open("rb") as source:
            version = tomllib.load(source)["package"]["version"]
    except FileNotFoundError as error:
        raise RuntimeError(f"Build from the complete Popu checkout; version source is missing: {manifest}") from error
    if not isinstance(version, str) or not version.strip():
        raise ValueError(f"{manifest}: package.version must be a non-empty string")
    return version


def generate_version(root: Path = ROOT) -> str:
    version = cargo_version(root)
    output = root / "python" / "src" / "popu" / "_version.py"
    data = f"# Generated from src-tauri/Cargo.toml; do not edit.\n__version__ = {version!r}\n"
    if output.is_file() and output.read_text(encoding="utf-8") == data:
        return version
    output.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(prefix="._version-", suffix=".tmp", dir=output.parent)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(data)
        temporary.replace(output)
    finally:
        temporary.unlink(missing_ok=True)
    return version


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--print", dest="print_only", action="store_true", help="Print the Cargo version without generating files")
    args = parser.parse_args()
    print(cargo_version() if args.print_only else generate_version())
