from __future__ import annotations

import os
from pathlib import Path
import runpy
import subprocess
from types import SimpleNamespace

import pytest

from project_paths import ROOT

builder = runpy.run_path(str(ROOT / "scripts/build-python-wheel.py"))


@pytest.fixture
def tools(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    resolve = builder["license_tool"]
    namespace = resolve.__globals__
    monkeypatch.setitem(namespace, "ROOT", tmp_path)
    monkeypatch.setenv("CARGO_HOME", str(tmp_path / "cargo home"))
    name = "cargo-about.exe" if os.name == "nt" else "cargo-about"
    cached = tmp_path / "python/build/tools/cargo-about-0.9.3/bin" / name
    state = SimpleNamespace(path=None, versions={}, installs=[], installed_version="0.9.3", failure=None,
                            cached=cached, standard=tmp_path / "cargo home/bin" / name)

    def binary(path: Path, version: str) -> Path:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"tool")
        state.versions[str(path)] = version
        return path

    def run(command, **kwargs):
        if command[:3] == ["cargo", "install", "cargo-about"]:
            state.installs.append((command, kwargs))
            if state.failure:
                raise state.failure
            binary(cached, state.installed_version)
            return subprocess.CompletedProcess(command, 0)
        assert command[1:] == ["--version"]
        return subprocess.CompletedProcess(command, 0, f"cargo-about {state.versions[command[0]]}\n")

    monkeypatch.setitem(namespace, "shutil", SimpleNamespace(which=lambda _: state.path))
    monkeypatch.setitem(namespace, "subprocess", SimpleNamespace(run=run, TimeoutExpired=subprocess.TimeoutExpired))
    state.binary = binary
    state.resolve = resolve
    return state


def test_matching_path_installation_is_reused(tools, tmp_path: Path) -> None:
    tools.path = str(tools.binary(tmp_path / "installed/tool.exe", "0.9.3"))
    assert tools.resolve() == Path(tools.path).resolve()
    assert not tools.installs


def test_standard_cargo_installation_can_be_found_without_path(tools) -> None:
    tools.binary(tools.standard, "0.9.3")
    assert tools.resolve() == tools.standard.resolve()
    assert not tools.installs


def test_correct_project_cache_is_used_instead_of_an_old_global_tool(tools, tmp_path: Path) -> None:
    tools.path = str(tools.binary(tmp_path / "old/tool.exe", "0.8.0"))
    tools.binary(tools.cached, "0.9.3")
    assert tools.resolve() == tools.cached.resolve()
    assert not tools.installs


@pytest.mark.parametrize("global_version", [None, "0.8.0"])
def test_missing_or_old_tool_is_prepared_once_in_the_project_cache(tools, tmp_path: Path, global_version: str | None) -> None:
    if global_version:
        tools.path = str(tools.binary(tmp_path / "global/tool.exe", global_version))
    assert tools.resolve() == tools.cached.resolve()
    assert tools.resolve() == tools.cached.resolve()
    assert len(tools.installs) == 1
    command, options = tools.installs[0]
    assert command == ["cargo", "install", "cargo-about", "--locked", "--version", "0.9.3", "--features", "cli", "--root", str(tools.cached.parent.parent)]
    assert options == {"cwd": tmp_path, "check": True}


def test_failed_installation_stops_instead_of_continuing_a_build(tools) -> None:
    tools.failure = subprocess.CalledProcessError(1, ["cargo", "install"])
    with pytest.raises(subprocess.CalledProcessError):
        tools.resolve()
    assert not tools.cached.exists()


def test_installation_is_checked_before_the_tool_can_be_used(tools) -> None:
    tools.installed_version = "0.8.0"
    with pytest.raises(RuntimeError, match="did not provide cargo-about 0.9.3"):
        tools.resolve()
