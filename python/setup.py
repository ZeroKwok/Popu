"""Setuptools configuration for a Python-ABI-independent Windows binary wheel."""
from __future__ import annotations

import hashlib
import json
import re
import shutil
import struct
from pathlib import Path

from setuptools import setup
from setuptools.command.bdist_wheel import bdist_wheel
from setuptools.command.build_py import build_py
from setuptools.errors import SetupError


def validate_payload(version: str) -> None:
    bundle = Path(__file__).parent / "src" / "popu" / "_native"
    try:
        manifest = json.loads((bundle / "manifest.json").read_text(encoding="utf-8"))
        if manifest["version"] != version or manifest["platform"] != "win_amd64" or "popu.exe" not in manifest["files"]:
            raise ValueError("payload version or platform mismatch")
        if not isinstance(manifest["files"], dict):
            raise ValueError("payload files must be a mapping")
        if set(path.name for path in bundle.iterdir()) != {*manifest["files"], "manifest.json"}:
            raise ValueError("payload file list does not match the manifest")
        for name, expected in manifest["files"].items():
            if not re.fullmatch(r"[A-Za-z0-9_.-]+", name) or name in (".", ".."):
                raise ValueError("invalid payload filename")
            if hashlib.sha256((bundle / name).read_bytes()).hexdigest() != expected:
                raise ValueError(f"checksum mismatch: {name}")
            if name.lower().endswith((".exe", ".dll")):
                with (bundle / name).open("rb") as stream:
                    if stream.read(2) != b"MZ":
                        raise ValueError("payload must be a Windows PE binary")
                    stream.seek(0x3C)
                    offset = struct.unpack("<I", stream.read(4))[0]
                    stream.seek(offset)
                    if stream.read(4) != b"PE\0\0" or struct.unpack("<H", stream.read(2))[0] != 0x8664:
                        raise ValueError("payload must target Windows x64")
    except (OSError, ValueError, KeyError, TypeError, struct.error) as error:
        raise SetupError("Native payload is missing or invalid. First run: python scripts/stage-python-runtime.py") from error


class NativeBuild(build_py):
    def run(self) -> None:
        if not self.editable_mode:
            validate_payload(self.distribution.get_version())
            # build_py otherwise leaves DLLs removed from a previous payload
            # in build/lib, which install_lib would silently ship in the wheel.
            root = Path(self.build_lib).resolve()
            destination = (root / "popu" / "_native").resolve()
            source = (Path(__file__).parent / "src" / "popu" / "_native").resolve()
            if not destination.is_relative_to(root) or destination == source:
                raise SetupError("Native build output must be separate from the source payload")
            if destination.exists() and not self.dry_run:
                shutil.rmtree(destination)
        super().run()


class WindowsWheel(bdist_wheel):
    def finalize_options(self) -> None:
        super().finalize_options()
        self.root_is_pure = False

    def get_tag(self) -> tuple[str, str, str]:
        return "py3", "none", "win_amd64"


setup(cmdclass={"build_py": NativeBuild, "bdist_wheel": WindowsWheel})
