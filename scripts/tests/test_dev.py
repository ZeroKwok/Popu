from __future__ import annotations

import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
from types import ModuleType, SimpleNamespace

import pytest

import dev
import project_paths


@pytest.fixture
def command(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> list[str]:
    stub = tmp_path / "native mock.py"
    stub.write_text('''
import json, os, sys
from pathlib import Path
request = Path(sys.argv[2])
with Path(os.environ["MOCK_LOG"]).open("a", encoding="utf-8") as stream:
    stream.write(json.dumps(sys.argv[1:], ensure_ascii=False) + "\\n")
raise SystemExit(json.loads(request.read_text(encoding="utf-8"))["exit"])
''', encoding="utf-8")
    monkeypatch.setenv("MOCK_LOG", str(tmp_path / "calls.jsonl"))
    return [sys.executable, str(stub)]


def request(root: Path, name: str, code: int) -> Path:
    path = root / name
    path.write_text(json.dumps({"exit": code}), encoding="utf-8")
    return path


def calls(root: Path) -> list[list[str]]:
    return [json.loads(line) for line in (root / "calls.jsonl").read_text(encoding="utf-8").splitlines()]


@pytest.mark.parametrize("code", [0, 10, 11, 12, 47])
def test_fixture_passes_native_exit_code_and_literal_template_arguments(tmp_path: Path, command: list[str], code: int) -> None:
    path = request(tmp_path, "request $(echo nope) 中文.json", code)
    template = 'templates/with spaces/"$HOME"'
    path.with_suffix(".template").write_bytes((template + "\r\nignored second line").encode("utf-8"))
    assert dev.run_fixture(command, path) == code
    assert calls(tmp_path) == [["request", str(path), "--template", template]]


def test_empty_sidecar_never_submits_a_request(tmp_path: Path, command: list[str]) -> None:
    path = request(tmp_path, "request.json", 0)
    path.with_suffix(".template").write_text("")
    with pytest.raises(ValueError, match="Empty template"):
        dev.run_fixture(command, path)
    assert not (tmp_path / "calls.jsonl").exists()


def test_batch_is_sorted_accepts_user_outcomes_and_stops_on_error(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, command: list[str]) -> None:
    monkeypatch.setattr(dev, "FIXTURES", tmp_path)
    for name, code in [("job-4.json", 12), ("job-2.json", 10), ("job-1.json", 0), ("job-3.json", 11), ("job-5.json", 0), ("other.json", 0)]:
        request(tmp_path, name, code)
    assert dev.run_fixtures(command, "job-") == 12
    assert [Path(value[1]).name for value in calls(tmp_path)] == ["job-1.json", "job-2.json", "job-3.json", "job-4.json"]


def test_batch_prefix_is_literal_and_empty_means_all(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, command: list[str]) -> None:
    monkeypatch.setattr(dev, "FIXTURES", tmp_path)
    request(tmp_path, "first.json", 10)
    request(tmp_path, "second.json", 11)
    with pytest.raises(ValueError, match="No protocol examples"):
        dev.run_fixtures(command, "*")
    assert dev.run_fixtures(command, "") == 0
    assert len(calls(tmp_path)) == 2


def test_fixture_lookup_accepts_names_and_explicit_paths(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(dev, "FIXTURES", tmp_path)
    path = request(tmp_path, "a.json", 0)
    assert dev.fixture_path("a.json") == path.resolve()
    assert dev.fixture_path(str(path)) == path.resolve()


@pytest.mark.parametrize("platform,name", [("nt", "popu.exe"), ("posix", "popu")])
def test_artifact_path_uses_cargo_metadata_for_custom_target_and_platform(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, platform: str, name: str) -> None:
    target = tmp_path / "custom target 中文"
    monkeypatch.setattr(project_paths, "os", SimpleNamespace(name=platform))
    seen = []
    def metadata(arguments, **kwargs):
        seen.append(arguments)
        return SimpleNamespace(stdout=json.dumps({"target_directory": str(target)}))
    monkeypatch.setattr(project_paths.subprocess, "run", metadata)
    assert project_paths.native_executable("release") == target / "release" / name
    assert "--no-deps" in seen[0]
    assert seen[0][-1] == str(project_paths.ROOT / "src-tauri/Cargo.toml")


def test_python_demo_runs_in_this_process_and_preserves_arguments(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    executable = tmp_path / "popu.exe"
    executable.touch()
    monkeypatch.setattr(dev, "native_executable", lambda: executable)
    observed = []
    module = ModuleType("popu.demo.__main__")
    def run(arguments):
        observed.append(arguments)
        return 130
    module.main = run
    monkeypatch.setitem(sys.modules, "popu.demo.__main__", module)
    with pytest.raises(SystemExit) as result:
        dev.python_demo(["--", "--wait"])
    assert result.value.code == 130
    assert observed == [["progress", "--executable", str(executable), "--wait"]]


@pytest.mark.parametrize("arguments", [["--help"], ["--executable", "some path/popU", "--wait"]])
def test_python_help_and_override_do_not_require_a_default_native_build(monkeypatch: pytest.MonkeyPatch, arguments: list[str]) -> None:
    monkeypatch.setattr(dev, "native_executable", lambda: pytest.fail("must not query Cargo"))
    seen = []
    module = ModuleType("popu.demo.__main__")
    module.main = lambda arguments: seen.append(arguments) or 0
    monkeypatch.setitem(sys.modules, "popu.demo.__main__", module)
    with pytest.raises(SystemExit) as result:
        dev.python_demo(arguments)
    assert result.value.code == 0
    assert seen == [["progress", *arguments]]


def test_windows_only_smoke_fails_before_querying_cargo(monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture) -> None:
    monkeypatch.setattr(project_paths, "sys", SimpleNamespace(platform="linux"))
    monkeypatch.setattr(dev, "native_executable", lambda: pytest.fail("must not query Cargo"))
    assert dev.main(["native-smoke"]) == 12
    assert "Windows only" in capsys.readouterr().err


def test_missing_native_binary_explains_build_prerequisite(tmp_path: Path) -> None:
    with pytest.raises(FileNotFoundError, match="just build"):
        project_paths.require_executable(tmp_path / "missing")


def test_icon_preparation_detects_missing_stale_and_current_outputs(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    module = runpy.run_path(str(project_paths.ROOT / "scripts/generate-icons.py"))
    check = module["icons_current"]
    namespace = check.__globals__
    source = tmp_path / "icon.svg"
    source.write_text("svg")
    monkeypatch.setitem(namespace, "SOURCE_SVG", source)
    monkeypatch.setitem(namespace, "ICON_DIR", tmp_path)
    monkeypatch.setitem(namespace, "TRAY_DIR", tmp_path / "tray")
    assert not check()
    (tmp_path / "tray").mkdir()
    outputs = [tmp_path / name for _, name in module["COMMITTED_PNGS"]]
    outputs += [tmp_path / "icon.ico"]
    outputs += [tmp_path / "tray" / f"{name}.png" for name, *_ in module["TRAY_FRAMES"]]
    future = max(source.stat().st_mtime, Path(namespace["__file__"]).stat().st_mtime) + 10
    for path in outputs:
        path.write_bytes(b"png")
        os.utime(path, (future, future))
    assert check()
    (tmp_path / "icon.ico").unlink()
    assert not check()
    assert check(tray_only=True)
    os.utime(source, (future + 10, future + 10))
    assert not check(tray_only=True)
