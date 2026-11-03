from __future__ import annotations

from pathlib import Path
import runpy

import pytest

builder = runpy.run_path(str(Path(__file__).parents[1] / "build_version.py"))


def manifest(root: Path, version: str) -> None:
    path = root / "src-tauri" / "Cargo.toml"
    path.parent.mkdir(exist_ok=True)
    path.write_text(f'[package]\nname = "popu"\nversion = "{version}"\n\n[dependencies]\nserde = "1"\n', encoding="utf-8")


def test_cargo_change_regenerates_sdk_version_without_a_second_manual_edit(tmp_path: Path) -> None:
    manifest(tmp_path, "1.2.3")
    assert builder["generate_version"](tmp_path) == "1.2.3"
    output = tmp_path / "python/src/popu/_version.py"
    assert runpy.run_path(str(output))["__version__"] == "1.2.3"
    manifest(tmp_path, "1.2.4-rc.1")
    assert builder["generate_version"](tmp_path) == "1.2.4-rc.1"
    assert runpy.run_path(str(output))["__version__"] == "1.2.4-rc.1"
    assert not list(output.parent.glob("._version-*.tmp"))


def test_unchanged_version_does_not_touch_the_generated_file(tmp_path: Path) -> None:
    manifest(tmp_path, "1.2.3")
    builder["generate_version"](tmp_path)
    output = tmp_path / "python/src/popu/_version.py"
    before = output.stat().st_mtime_ns
    builder["generate_version"](tmp_path)
    assert output.stat().st_mtime_ns == before


def test_missing_version_source_fails_instead_of_using_a_stale_generated_version(tmp_path: Path) -> None:
    manifest(tmp_path, "1.2.3")
    builder["generate_version"](tmp_path)
    (tmp_path / "src-tauri/Cargo.toml").unlink()
    with pytest.raises(RuntimeError, match="complete Popu checkout"):
        builder["generate_version"](tmp_path)
