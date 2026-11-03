from __future__ import annotations

import hashlib
import json
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest

from popu import Client, PopuError
from popu import _runtime as runtime


@pytest.fixture
def bundle(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    source = tmp_path / "package with spaces" / "_native"
    source.mkdir(parents=True)
    monkeypatch.setattr(runtime, "_bundle_directory", lambda: source)
    monkeypatch.setattr(runtime, "_cache_directory", lambda: tmp_path / "用户 cache" / "runtime")
    return source


def payload(source: Path, data: bytes = b"native binary") -> dict:
    (source / "popu.exe").write_bytes(data)
    manifest = {"version": runtime.__version__, "platform": "win_amd64", "files": {"popu.exe": hashlib.sha256(data).hexdigest()}}
    (source / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
    return manifest


def test_default_client_uses_verified_cache_instead_of_path(bundle: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    payload(bundle)
    monkeypatch.setenv("PATH", str(bundle / "unrelated executable"))
    executable = runtime.executable_path()
    assert executable != bundle / "popu.exe"
    assert executable.read_bytes() == b"native binary"
    assert Client()._command == [str(executable)]
    assert not list(executable.parent.parent.glob(".popu-*"))


def test_runtime_publication_is_safe_for_concurrent_callers(bundle: Path) -> None:
    payload(bundle)
    with ThreadPoolExecutor(max_workers=8) as pool:
        paths = list(pool.map(lambda _: runtime.executable_path(), range(24)))
    assert len(set(paths)) == 1
    assert paths[0].read_bytes() == b"native binary"
    assert not list(paths[0].parent.parent.glob(".popu-*"))


def test_upgrade_preserves_an_open_old_runtime(bundle: Path) -> None:
    payload(bundle, b"old binary")
    old = runtime.executable_path()
    with old.open("rb") as opened:
        payload(bundle, b"new binary")
        new = runtime.executable_path()
        assert new != old
        assert opened.read() == b"old binary"
        assert old.read_bytes() == b"old binary"
        assert new.read_bytes() == b"new binary"


def test_corrupt_cache_is_rejected_without_overwriting_it(bundle: Path) -> None:
    payload(bundle)
    executable = runtime.executable_path()
    executable.write_bytes(b"damaged")
    with pytest.raises(PopuError, match="checksum mismatch"):
        runtime.executable_path()
    assert executable.read_bytes() == b"damaged"


def test_corrupt_payload_is_never_published(bundle: Path) -> None:
    payload(bundle)
    (bundle / "popu.exe").write_bytes(b"damaged")
    with pytest.raises(PopuError, match="checksum mismatch"):
        runtime.executable_path()
    root = runtime._cache_directory()
    assert not list(root.iterdir())


@pytest.mark.parametrize("change", ["version", "platform", "filename", "digest"])
def test_invalid_manifests_are_rejected(bundle: Path, change: str) -> None:
    manifest = payload(bundle)
    if change in ("version", "platform"):
        manifest[change] = "incorrect"
    elif change == "filename":
        manifest["files"]["../escape.exe"] = "0" * 64
    else:
        manifest["files"]["popu.exe"] = "invalid"
    (bundle / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
    with pytest.raises(PopuError):
        runtime.executable_path()
    assert not runtime._cache_directory().exists()


def test_missing_bundle_explains_source_development_override(bundle: Path) -> None:
    with pytest.raises(PopuError, match="Client\\(executable="):
        runtime.executable_path()
    assert Client(executable="custom.exe")._command == ["custom.exe"]


def test_unsupported_platform_does_not_prepare_a_cache(bundle: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(runtime.sys, "platform", "linux")
    with pytest.raises(PopuError, match="Windows only"):
        runtime.executable_path()
    assert not runtime._cache_directory().exists()
