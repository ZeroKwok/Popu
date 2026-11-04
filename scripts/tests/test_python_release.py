from __future__ import annotations

import hashlib
import base64
import csv
import io
import json
from pathlib import Path
import runpy
import struct
import zipfile

import pytest

from native_payload import imported_dlls, verify_portable_binary
from project_paths import ROOT

release = runpy.run_path(str(ROOT / "scripts/verify-python-release.py"))


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
    (tmp_path / "LICENSE").write_bytes(b"project license")
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
        "popu-0.1.1.dist-info/licenses/LICENSE": b"project license",
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
