from __future__ import annotations

import hashlib
import base64
import csv
import io
import json
import os
from pathlib import Path
import runpy
import struct
import subprocess
import sys
import zipfile

import pytest

from native_payload import imported_dlls, verify_portable_binary
from project_paths import ROOT

release = runpy.run_path(str(ROOT / "scripts/verify-python-release.py"))
staging = runpy.run_path(str(ROOT / "scripts/stage-python-runtime.py"))


def pe(dll: str = "kernel32.dll", *, delay: str | None = None) -> bytes:
    """Minimal PE32+ image with real RVA-based import descriptors."""
    data = bytearray(1024)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<HH", data, 0x84, 0x8664, 1)
    struct.pack_into("<H", data, 0x94, 240)
    struct.pack_into("<H", data, 0x98, 0x20B)
    struct.pack_into("<Q", data, 0xB0, 0x140000000)
    struct.pack_into("<I", data, 0xD4, 512)
    struct.pack_into("<I", data, 0x104, 16)
    struct.pack_into("<II", data, 0x110, 0x1000, 40)
    struct.pack_into("<IIII", data, 0x190, 512, 0x1000, 512, 512)
    struct.pack_into("<I", data, 0x20C, 0x1050)
    encoded = dll.encode() + b"\0"
    data[0x250:0x250 + len(encoded)] = encoded
    if delay:
        struct.pack_into("<II", data, 0x170, 0x1080, 64)
        struct.pack_into("<II", data, 0x280, 1, 0x10D0)
        encoded = delay.encode() + b"\0"
        data[0x2D0:0x2D0 + len(encoded)] = encoded
    return bytes(data)


def test_portable_pe_can_be_inspected_without_visual_studio() -> None:
    assert verify_portable_binary(pe("KERNEL32.dll")) == {"kernel32.dll"}
    assert imported_dlls(pe(delay="USER32.dll")) == {"kernel32.dll", "user32.dll"}


@pytest.mark.parametrize("dll", ["VCRUNTIME140.dll", "VCRUNTIME140_1.dll", "MSVCP140.dll", "CONCRT140.dll"])
@pytest.mark.parametrize("delay", [False, True])
def test_vc_runtime_dependencies_are_rejected_even_when_delay_loaded(dll: str, delay: bool) -> None:
    data = pe(delay=dll) if delay else pe(dll)
    with pytest.raises(ValueError, match="static CRT"):
        verify_portable_binary(data)


@pytest.mark.parametrize("change", ["truncated", "signature", "architecture", "rva", "terminator"])
def test_malformed_or_wrong_platform_binaries_do_not_pass_release_checks(change: str) -> None:
    data = bytearray(pe())
    if change == "truncated":
        data = data[:40]
    elif change == "signature":
        data[:2] = b"NO"
    elif change == "architecture":
        struct.pack_into("<H", data, 0x84, 0x14C)
    elif change == "rva":
        struct.pack_into("<I", data, 0x20C, 0xFFFF)
    else:
        data[0x250:] = b"x" * (len(data) - 0x250)
    with pytest.raises(ValueError):
        imported_dlls(bytes(data))


@pytest.mark.parametrize(("version", "normalized"), [("0.1.1", "0.1.1"), ("1.2.3-rc.1", "1.2.3rc1")])
def test_release_tag_and_wheel_version_share_the_cargo_source(monkeypatch: pytest.MonkeyPatch, version: str, normalized: str) -> None:
    check = release["release_version"]
    monkeypatch.setitem(check.__globals__, "cargo_version", lambda: version)
    assert check(f"v{version}") == (version, normalized)
    with pytest.raises(ValueError, match="Release tag"):
        check("v9.9.9")


def test_local_build_metadata_cannot_accidentally_be_uploaded_to_pypi(monkeypatch: pytest.MonkeyPatch) -> None:
    check = release["release_version"]
    monkeypatch.setitem(check.__globals__, "cargo_version", lambda: "0.1.1+local")
    with pytest.raises(ValueError, match="build metadata"):
        check()


@pytest.fixture
def wheel_files(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> dict[str, bytes]:
    (tmp_path / "src-tauri").mkdir()
    (tmp_path / "LICENSE").write_bytes(b"project license\n")
    (tmp_path / "src-tauri/Cargo.lock").write_bytes(b"lockfile")
    monkeypatch.setitem(release["inspect_wheel"].__globals__, "ROOT", tmp_path)
    files = {
        "popu.exe": pe(), "LICENSE": b"project license", "THIRD_PARTY_LICENSES.txt": b"UI license",
        "RUST_THIRD_PARTY_LICENSES.txt": f"Cargo.lock SHA-256: {hashlib.sha256(b'lockfile').hexdigest()}\nNative licenses".encode(),
    }
    manifest = {"version":"0.1.1", "platform":"win_amd64", "files":{name:hashlib.sha256(data).hexdigest() for name,data in files.items()}}
    return {
        "popu-0.1.1.dist-info/METADATA": b"Name: popu\nVersion: 0.1.1\nRequires-Python: >=3.10\n",
        "popu-0.1.1.dist-info/WHEEL": b"Root-Is-Purelib: false\nTag: py3-none-win_amd64\n",
        "popu-0.1.1.dist-info/entry_points.txt": b"[console_scripts]\npopu = popu._cli:main\n",
        "popu-0.1.1.dist-info/licenses/LICENSE": b"project license\n",
        "popu/_version.py": b"__version__ = '0.1.1'\n",
        "popu/demo/progress.py": b"demo",
        "popu/_native/manifest.json": json.dumps(manifest).encode(),
        **{f"popu/_native/{name}":data for name,data in files.items()},
    }


def write_wheel(path: Path, files: dict[str, bytes]) -> None:
    record = io.StringIO()
    writer = csv.writer(record)
    for name, data in files.items():
        digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
        writer.writerow([name, f"sha256={digest}", len(data)])
    writer.writerow(["popu-0.1.1.dist-info/RECORD", "", ""])
    with zipfile.ZipFile(path, "w") as archive:
        for name, data in files.items():
            archive.writestr(name, data)
        archive.writestr("popu-0.1.1.dist-info/RECORD", record.getvalue())


def test_complete_wheel_passes_content_and_record_validation(tmp_path: Path, wheel_files: dict) -> None:
    wheel = tmp_path / "popu-0.1.1-py3-none-win_amd64.whl"
    write_wheel(wheel, wheel_files)
    report = release["inspect_wheel"](wheel, "0.1.1", "0.1.1")
    assert report["native_sha256"] == hashlib.sha256(pe()).hexdigest()
    assert report["imports"] == {"popu.exe":["kernel32.dll"]}


@pytest.mark.parametrize(("source", "packaged"), [(b"\n", b"\r\n"), (b"\r\n", b"\n")])
def test_license_verification_accepts_git_checkout_line_ending_differences(tmp_path: Path, wheel_files: dict, source: bytes, packaged: bytes) -> None:
    (tmp_path / "LICENSE").write_bytes(b"project license" + source)
    wheel_files["popu-0.1.1.dist-info/licenses/LICENSE"] = b"project license" + packaged
    wheel = tmp_path / "popu-0.1.1-py3-none-win_amd64.whl"
    write_wheel(wheel, wheel_files)
    assert release["inspect_wheel"](wheel, "0.1.1", "0.1.1")["version"] == "0.1.1"


def test_wheel_license_content_changes_are_rejected_with_a_valid_record(tmp_path: Path, wheel_files: dict) -> None:
    wheel_files["popu-0.1.1.dist-info/licenses/LICENSE"] = b"different license\r\n"
    wheel = tmp_path / "popu-0.1.1-py3-none-win_amd64.whl"
    write_wheel(wheel, wheel_files)
    with pytest.raises(ValueError, match="Wheel license"):
        release["inspect_wheel"](wheel, "0.1.1", "0.1.1")


@pytest.fixture
def stage_project(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    namespace = staging["main"].__globals__
    sources = {
        "LICENSE": b"project license\nsecond line\n",
        "python/LICENSE": b"project license\nsecond line\n",
        "src-tauri/Cargo.lock": b"lockfile",
        "ui/dist/vendor/THIRD_PARTY_LICENSES.txt": b"UI licenses",
        "python/build/RUST_THIRD_PARTY_LICENSES.txt": f"Cargo.lock SHA-256: {hashlib.sha256(b'lockfile').hexdigest()}\nRust licenses".encode(),
        "native/popu.exe": pe(),
    }
    for name, data in sources.items():
        path = tmp_path / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    monkeypatch.setitem(namespace, "ROOT", tmp_path)
    monkeypatch.setitem(namespace, "generate_version", lambda: "0.1.1")
    monkeypatch.setitem(namespace, "require_windows", lambda _: None)
    monkeypatch.setitem(namespace, "native_executable", lambda *args, **kwargs: tmp_path / "native/popu.exe")
    monkeypatch.setattr(subprocess, "run", lambda command, **kwargs: subprocess.CompletedProcess(command, 0, "popu 0.1.1\n"))
    return tmp_path


@pytest.mark.parametrize(("root_license", "python_license"), [
    (b"project license\nsecond line\n", b"project license\r\nsecond line\r\n"),
    (b"project license\r\nsecond line\r\n", b"project license\nsecond line\n"),
    (b"project license\r\nsecond line\n", b"project license\nsecond line\r\n"),
])
def test_staging_accepts_license_line_endings_and_hashes_original_bytes(stage_project: Path, root_license: bytes, python_license: bytes) -> None:
    (stage_project / "LICENSE").write_bytes(root_license)
    (stage_project / "python/LICENSE").write_bytes(python_license)
    staging["main"]([])
    bundle = stage_project / "python/src/popu/_native"
    assert (bundle / "LICENSE").read_bytes() == root_license
    assert (stage_project / "python/LICENSE").read_bytes() == python_license
    manifest = json.loads((bundle / "manifest.json").read_text())
    assert manifest["files"]["LICENSE"] == hashlib.sha256(root_license).hexdigest()


@pytest.mark.parametrize("python_license", [
    b"different license\r\nsecond line\r\n",
    b"project license \r\nsecond line\r\n",
    b"\xef\xbb\xbfproject license\r\nsecond line\r\n",
])
def test_staging_rejects_real_license_changes_before_replacing_a_payload(stage_project: Path, python_license: bytes) -> None:
    (stage_project / "python/LICENSE").write_bytes(python_license)
    bundle = stage_project / "python/src/popu/_native"
    bundle.mkdir(parents=True)
    marker = bundle / "previous-payload"
    marker.write_bytes(b"keep")
    with pytest.raises(ValueError, match="python/LICENSE"):
        staging["main"]([])
    assert marker.read_bytes() == b"keep"
    assert not (bundle / "manifest.json").exists()


@pytest.mark.parametrize("dev", [False, True])
def test_staging_selects_the_matching_cargo_build(stage_project: Path, monkeypatch: pytest.MonkeyPatch, dev: bool) -> None:
    calls = []

    def executable(*args, **kwargs):
        calls.append((args, kwargs))
        return stage_project / "native/popu.exe"

    monkeypatch.setitem(staging["main"].__globals__, "native_executable", executable)
    staging["main"](["--dev"] if dev else [])
    expected = ((), {}) if dev else (("release",), {"target": "x86_64-pc-windows-msvc"})
    assert calls == [expected]
    manifest = json.loads((stage_project / "python/src/popu/_native/manifest.json").read_text())
    assert manifest.get("development", False) is dev


def test_dev_staging_only_needs_binaries_and_replaces_stale_release_files(stage_project: Path) -> None:
    staging["main"]([])
    for name in ("LICENSE", "python/LICENSE", "src-tauri/Cargo.lock", "ui/dist/vendor/THIRD_PARTY_LICENSES.txt", "python/build/RUST_THIRD_PARTY_LICENSES.txt"):
        (stage_project / name).unlink()
    binary = pe("VCRUNTIME140D.dll")
    (stage_project / "native/popu.exe").write_bytes(binary)
    library = stage_project / "native/helper.dll"
    library.write_bytes(pe("MSVCP140D.dll"))
    staging["main"](["--dev", "--executable", str(stage_project / "native/popu.exe"), "--dll", str(library)])
    bundle = stage_project / "python/src/popu/_native"
    manifest = json.loads((bundle / "manifest.json").read_text())
    assert set(path.name for path in bundle.iterdir()) == {"popu.exe", "helper.dll", "manifest.json"}
    assert manifest["development"] is True
    assert manifest["files"] == {"popu.exe": hashlib.sha256(binary).hexdigest(), "helper.dll": hashlib.sha256(library.read_bytes()).hexdigest()}


@pytest.mark.parametrize("change", ["architecture", "version"])
def test_dev_staging_validates_before_replacing_payload(stage_project: Path, monkeypatch: pytest.MonkeyPatch, change: str) -> None:
    staging["main"]([])
    bundle = stage_project / "python/src/popu/_native"
    original = (bundle / "manifest.json").read_bytes()
    if change == "architecture":
        binary = bytearray(pe())
        struct.pack_into("<H", binary, 0x84, 0x14C)
        (stage_project / "native/popu.exe").write_bytes(binary)
    else:
        monkeypatch.setattr(subprocess, "run", lambda command, **kwargs: subprocess.CompletedProcess(command, 0, "popu 9.9.9\n"))
    with pytest.raises(ValueError):
        staging["main"](["--dev"])
    assert (bundle / "manifest.json").read_bytes() == original


def test_release_staging_still_rejects_dynamic_crt(stage_project: Path) -> None:
    (stage_project / "native/popu.exe").write_bytes(pe("VCRUNTIME140.dll"))
    with pytest.raises(ValueError, match="static CRT"):
        staging["main"]([])


def test_release_staging_replaces_development_manifest_and_files(stage_project: Path) -> None:
    library = stage_project / "native/helper.dll"
    library.write_bytes(pe())
    staging["main"](["--dev", "--dll", str(library)])
    staging["main"]([])
    bundle = stage_project / "python/src/popu/_native"
    manifest = json.loads((bundle / "manifest.json").read_text())
    assert "development" not in manifest
    assert set(manifest["files"]) == {"popu.exe", "LICENSE", "THIRD_PARTY_LICENSES.txt", "RUST_THIRD_PARTY_LICENSES.txt"}
    assert not (bundle / "helper.dll").exists()


@pytest.mark.parametrize("dev", [False, True])
def test_wheel_build_rejects_dev_payload_but_accepts_release(stage_project: Path, monkeypatch: pytest.MonkeyPatch, dev: bool) -> None:
    import build_version
    import setuptools
    from setuptools.errors import SetupError

    monkeypatch.setattr(setuptools, "setup", lambda **kwargs: None)
    monkeypatch.setattr(build_version, "generate_version", lambda: "0.1.1")
    config = runpy.run_path(str(ROOT / "python/setup.py"))
    validate = config["validate_payload"]
    monkeypatch.setitem(validate.__globals__, "__file__", str(stage_project / "python/setup.py"))
    staging["main"](["--dev"] if dev else [])
    if dev:
        with pytest.raises(SetupError, match="Development native payload.*just build-wheel"):
            validate("0.1.1")
    else:
        validate("0.1.1")


def test_release_verifier_rejects_dev_payload_even_with_complete_licenses(tmp_path: Path, wheel_files: dict) -> None:
    manifest = json.loads(wheel_files["popu/_native/manifest.json"])
    manifest["development"] = True
    wheel_files["popu/_native/manifest.json"] = json.dumps(manifest).encode()
    wheel = tmp_path / "popu-0.1.1-py3-none-win_amd64.whl"
    write_wheel(wheel, wheel_files)
    with pytest.raises(ValueError, match="Development native payload"):
        release["inspect_wheel"](wheel, "0.1.1", "0.1.1")


@pytest.mark.parametrize("change", ["checksum", "missing_license", "platform", "stale_license"])
def test_invalid_wheel_is_rejected_even_with_a_correct_record(tmp_path: Path, wheel_files: dict, change: str) -> None:
    if change == "checksum":
        wheel_files["popu/_native/popu.exe"] = pe("user32.dll")
    elif change == "missing_license":
        del wheel_files["popu/_native/RUST_THIRD_PARTY_LICENSES.txt"]
    elif change == "platform":
        wheel_files["popu-0.1.1.dist-info/WHEEL"] = b"Root-Is-Purelib: true\nTag: py3-none-any\n"
    else:
        manifest = json.loads(wheel_files["popu/_native/manifest.json"])
        value = b"Cargo.lock SHA-256: stale\n"
        wheel_files["popu/_native/RUST_THIRD_PARTY_LICENSES.txt"] = value
        manifest["files"]["RUST_THIRD_PARTY_LICENSES.txt"] = hashlib.sha256(value).hexdigest()
        wheel_files["popu/_native/manifest.json"] = json.dumps(manifest).encode()
    wheel = tmp_path / "popu-0.1.1-py3-none-win_amd64.whl"
    write_wheel(wheel, wheel_files)
    with pytest.raises(ValueError):
        release["inspect_wheel"](wheel, "0.1.1", "0.1.1")


def test_installation_identity_still_rejects_paths_outside_the_selected_roots(tmp_path: Path) -> None:
    root = tmp_path / "venv"
    root.mkdir()
    assert release["_is_within"](root / "Lib/site-packages/popu", root)
    assert not release["_is_within"](tmp_path / "venv-other/Lib/site-packages/popu", root)


@pytest.mark.skipif(os.name != "nt", reason="Windows 8.3 path aliases")
def test_installation_identity_accepts_short_and_long_spellings_of_the_same_root(tmp_path: Path) -> None:
    import ctypes
    from ctypes import wintypes
    root = (tmp_path / "long installation directory").resolve()
    root.mkdir()
    get_short = ctypes.WinDLL("kernel32", use_last_error=True).GetShortPathNameW
    get_short.argtypes = [wintypes.LPCWSTR, wintypes.LPWSTR, wintypes.DWORD]
    get_short.restype = wintypes.DWORD
    buffer = ctypes.create_unicode_buffer(32768)
    length = get_short(str(root), buffer, len(buffer))
    assert 0 < length < len(buffer)
    alias = Path(buffer.value)
    if alias == root:
        pytest.skip("This filesystem has no short alias for the temporary directory")
    assert not (root / "popu").is_relative_to(alias)
    assert release["_is_within"](root / "popu", alias)
    assert release["_is_within"](alias / "popu", root)


def test_wheel_lookup_uses_the_expected_version_or_an_explicit_path(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    resolve = release["wheel_path"]
    monkeypatch.setitem(resolve.__globals__, "ROOT", tmp_path)
    dist = tmp_path / "dist"
    dist.mkdir()
    (dist / "popu-0.1.0-py3-none-win_amd64.whl").touch()
    with pytest.raises(FileNotFoundError, match="just build-wheel"):
        resolve(None, "1.2.3rc1")
    expected = dist / "popu-1.2.3rc1-py3-none-win_amd64.whl"
    expected.touch()
    assert resolve(None, "1.2.3rc1") == expected.resolve()
    explicit = tmp_path / "download with spaces.whl"
    explicit.touch()
    assert resolve(explicit, "1.2.3rc1") == explicit.resolve()


@pytest.mark.parametrize("mode", ["verify", "test", "setup"])
def test_wheel_commands_share_validation_and_keep_install_targets_separate(tmp_path: Path, wheel_files: dict, monkeypatch: pytest.MonkeyPatch, mode: str) -> None:
    main = release["main"]
    namespace = main.__globals__
    monkeypatch.setitem(namespace, "cargo_version", lambda: "0.1.1")
    monkeypatch.setitem(namespace, "require_windows", lambda _: None)
    wheel = tmp_path / "dist/popu-0.1.1-py3-none-win_amd64.whl"
    wheel.parent.mkdir()
    write_wheel(wheel, wheel_files)
    calls = []
    monkeypatch.setattr(subprocess, "run", lambda arguments, **kwargs: calls.append((arguments, kwargs)))
    smoke_calls = []
    def smoke(path, report):
        smoke_calls.append(path)
        report["isolated_install"] = "passed"
    monkeypatch.setitem(namespace, "smoke_install", smoke)
    report = tmp_path / "reports/wheel.json"
    arguments = ["--report", str(report)]
    if mode == "setup":
        arguments += ["--install"]
    else:
        arguments += ["--twine"]
        if mode == "test":
            arguments += ["--smoke"]
    main(arguments)
    assert json.loads(report.read_text())["sha256"] == hashlib.sha256(wheel.read_bytes()).hexdigest()
    if mode == "setup":
        assert calls == [([sys.executable, "-m", "pip", "install", "--force-reinstall", "--no-index", "--no-deps", str(wheel.resolve())], {"check": True})]
    else:
        assert calls == [([sys.executable, "-m", "twine", "check", "--strict", str(wheel.resolve())], {"check": True})]
    assert smoke_calls == ([wheel.resolve()] if mode == "test" else [])


def test_invalid_payload_prevents_installing_into_the_current_environment(tmp_path: Path, wheel_files: dict, monkeypatch: pytest.MonkeyPatch) -> None:
    main = release["main"]
    monkeypatch.setitem(main.__globals__, "cargo_version", lambda: "0.1.1")
    monkeypatch.setattr(subprocess, "run", lambda *args, **kwargs: pytest.fail("invalid wheel must not reach pip"))
    wheel_files["popu/_native/popu.exe"] = pe("user32.dll")
    wheel = tmp_path / "popu-0.1.1-py3-none-win_amd64.whl"
    write_wheel(wheel, wheel_files)
    with pytest.raises(ValueError, match="checksum"):
        main(["--install", "--wheel", str(wheel)])


@pytest.mark.parametrize("mode", ["--smoke", "--install"])
def test_failed_metadata_check_prevents_installation(tmp_path: Path, wheel_files: dict, monkeypatch: pytest.MonkeyPatch, mode: str) -> None:
    main = release["main"]
    monkeypatch.setitem(main.__globals__, "cargo_version", lambda: "0.1.1")
    monkeypatch.setitem(main.__globals__, "smoke_install", lambda *args: pytest.fail("metadata validation failed"))
    monkeypatch.setitem(main.__globals__, "install_wheel", lambda *args: pytest.fail("metadata validation failed"))
    def fail(arguments, **kwargs):
        raise subprocess.CalledProcessError(1, arguments)
    monkeypatch.setattr(subprocess, "run", fail)
    wheel = tmp_path / "popu-0.1.1-py3-none-win_amd64.whl"
    write_wheel(wheel, wheel_files)
    with pytest.raises(subprocess.CalledProcessError):
        main([mode, "--twine", "--wheel", str(wheel)])


def test_current_and_isolated_install_modes_cannot_be_combined() -> None:
    with pytest.raises(SystemExit) as result:
        release["main"](["--smoke", "--install"])
    assert result.value.code == 2
