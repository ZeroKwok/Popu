"""Validate release identity, wheel contents, and an isolated Windows install."""
from __future__ import annotations

import argparse
import ast
import base64
import csv
from email.parser import BytesParser
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tempfile
import venv
import zipfile

from packaging.utils import parse_wheel_filename
from packaging.version import Version

from native_payload import verify_portable_binary
from project_paths import ROOT, require_windows

sys.path.insert(0, str(ROOT / "python"))
from build_version import cargo_version


def release_version(tag: str | None = None) -> tuple[str, str]:
    native = cargo_version()
    python = Version(native)
    if python.local is not None:
        raise ValueError("PyPI releases cannot contain SemVer build metadata (+...)")
    if tag is not None and tag != f"v{native}":
        raise ValueError(f"Release tag {tag!r} must equal v{native} from src-tauri/Cargo.toml")
    return native, str(python)


def inspect_wheel(path: Path, native_version: str, python_version: str) -> dict:
    name, version, _, tags = parse_wheel_filename(path.name)
    if name != "popu" or str(version) != python_version or {str(tag) for tag in tags} != {"py3-none-win_amd64"}:
        raise ValueError(f"Unexpected wheel identity: {path.name}")
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        if len(names) != len(set(names)) or any("\\" in name or PurePosixPath(name).is_absolute() or ".." in PurePosixPath(name).parts for name in names):
            raise ValueError("Wheel has duplicate or unsafe paths")

        def find(suffix: str) -> str:
            matches = [name for name in names if name.endswith(suffix)]
            if len(matches) != 1:
                raise ValueError(f"Expected one wheel member ending in {suffix}, found {matches}")
            return matches[0]

        metadata = BytesParser().parsebytes(archive.read(find(".dist-info/METADATA")))
        if metadata["Name"] != "popu" or Version(metadata["Version"]) != Version(python_version) or metadata["Requires-Python"] != ">=3.10":
            raise ValueError("Wheel metadata does not match the project/version/Python requirement")
        wheel = BytesParser().parsebytes(archive.read(find(".dist-info/WHEEL")))
        if wheel["Root-Is-Purelib"] != "false" or wheel.get_all("Tag") != ["py3-none-win_amd64"]:
            raise ValueError("Wheel must be platform specific and Python ABI independent")
        if "popu = popu._cli:main" not in archive.read(find(".dist-info/entry_points.txt")).decode():
            raise ValueError("Missing popu console entry point")
        version_tree = ast.parse(archive.read(find("popu/_version.py")))
        embedded = [ast.literal_eval(node.value) for node in version_tree.body if isinstance(node, ast.Assign) and any(isinstance(target, ast.Name) and target.id == "__version__" for target in node.targets)]
        if embedded != [native_version]:
            raise ValueError("SDK version does not match the native application version")
        find("popu/demo/progress.py")
        # Git checkouts can choose LF or CRLF independently of the builder.
        if archive.read(find(".dist-info/licenses/LICENSE")).replace(b"\r\n", b"\n") != (ROOT / "LICENSE").read_bytes().replace(b"\r\n", b"\n"):
            raise ValueError("Wheel license does not match the repository license")

        manifest_path = find("popu/_native/manifest.json")
        prefix = manifest_path.removesuffix("manifest.json")
        manifest = json.loads(archive.read(manifest_path))
        if manifest["version"] != native_version or manifest["platform"] != "win_amd64":
            raise ValueError("Native manifest version/platform mismatch")
        files = manifest["files"]
        required = {"popu.exe", "LICENSE", "THIRD_PARTY_LICENSES.txt", "RUST_THIRD_PARTY_LICENSES.txt"}
        if not required <= set(files) or {name.removeprefix(prefix) for name in names if name.startswith(prefix)} != {*files, "manifest.json"}:
            raise ValueError("Native payload is incomplete or has files outside its manifest")
        imports = {}
        for name, expected in files.items():
            if not re.fullmatch(r"[A-Za-z0-9_.-]+", name) or name in (".", ".."):
                raise ValueError("Invalid native payload name")
            data = archive.read(prefix + name)
            if hashlib.sha256(data).hexdigest() != expected:
                raise ValueError(f"Native checksum mismatch: {name}")
            if name.lower().endswith((".exe", ".dll")):
                imports[name] = sorted(verify_portable_binary(data))
        lock_hash = hashlib.sha256((ROOT / "src-tauri/Cargo.lock").read_bytes()).hexdigest()
        if not archive.read(prefix + "RUST_THIRD_PARTY_LICENSES.txt").decode().startswith(f"Cargo.lock SHA-256: {lock_hash}\n"):
            raise ValueError("Rust license notices do not match Cargo.lock")

        record_path = find(".dist-info/RECORD")
        rows = list(csv.reader(io.StringIO(archive.read(record_path).decode())))
        if len(rows) != len(names) or {row[0] for row in rows} != set(names):
            raise ValueError("Wheel RECORD does not cover exactly its files")
        for name, digest, size in rows:
            if name == record_path:
                if digest or size:
                    raise ValueError("RECORD cannot hash itself")
                continue
            data = archive.read(name)
            expected = "sha256=" + base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
            if digest != expected or int(size) != len(data):
                raise ValueError(f"Invalid wheel RECORD entry: {name}")
    return {"wheel": path.name, "version": native_version, "python_version": python_version,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "native_sha256": files["popu.exe"], "imports": imports}


def _is_within(path: Path, root: Path) -> bool:
    return path.resolve().is_relative_to(root.resolve())


def smoke_install(path: Path, report: dict) -> None:
    require_windows("Installed wheel verification")
    # Keep all runtime/cache/cwd state outside the checkout's import paths.
    with tempfile.TemporaryDirectory(prefix="popu-wheel-review-") as temporary:
        # Windows runner TEMP can use an 8.3 alias such as RUNNER~1. The SDK
        # resolves its paths, so use the same canonical spelling for roots.
        work = Path(temporary).resolve()
        home = work / "venv"
        venv.EnvBuilder(with_pip=True).create(home)
        python = home / "Scripts/python.exe"
        cli = home / "Scripts/popu.exe"
        environment = {key: value for key, value in os.environ.items() if key not in {"PYTHONPATH", "PYTHONHOME"}}
        environment.update(LOCALAPPDATA=str(work / "cache \u4e2d\u6587"), PYTHONIOENCODING="utf-8",
                           PATH=os.pathsep.join([str(home / "Scripts"), str(Path(os.environ["SYSTEMROOT"]) / "System32")]))

        def run(arguments: list, *, expected: int = 0, input: str | None = None, env: dict | None = None):
            result = subprocess.run([str(value) for value in arguments], cwd=work, env=env or environment,
                                    capture_output=True, text=True, encoding="utf-8", input=input, timeout=60)
            if result.returncode != expected:
                raise RuntimeError(f"{arguments}: exit {result.returncode}, expected {expected}\n{result.stdout}\n{result.stderr}")
            return result

        run([python, "-m", "pip", "install", "--no-index", "--no-deps", path])
        for command in ([cli], [python, "-m", "popu"]):
            if run([*command, "--version"]).stdout.strip() != f"popu {report['version']}":
                raise ValueError("Installed CLI returned the wrong version")
            if json.loads(run([*command, "templates", "list"]).stdout) != ["confirm", "form", "notification"]:
                raise ValueError("Installed CLI cannot read its embedded templates")
            failed = run([*command, "request", "-"], expected=12, input='{"invalid":"\u4e2d\u6587"}')
            if failed.stdout or "Popu error:" not in failed.stderr:
                raise ValueError("CLI failed to preserve validation stderr/stdin/exit code")
            empty = work / "no-webview"
            empty.mkdir(exist_ok=True)
            failed = run([*command, "host", "run"], expected=12, env={**environment, "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER": str(empty)})
            if failed.stdout or "WebView2 Runtime is unavailable" not in failed.stderr or "https://developer.microsoft.com/microsoft-edge/webview2/" not in failed.stderr:
                raise ValueError("Missing-WebView2 diagnostic was lost by the installed CLI")
        for args in (["--help"], ["progress", "--help"]):
            run([python, "-m", "popu.demo", *args])
        probe = r'''
import hashlib, json
from pathlib import Path
import popu
from popu import Client, PopuError
client = Client()
try:
    client.notify({'invalid':'\u4e2d\u6587'})
except PopuError as error:
    assert 'Popu error:' in str(error)
else:
    raise AssertionError('invalid request accepted')
native = Path(client._command[0])
print(json.dumps({'package':str(Path(popu.__file__).resolve()),'version':popu.__version__,
                  'native':str(native),'sha256':hashlib.sha256(native.read_bytes()).hexdigest()}))
'''
        installed = json.loads(run([python, "-c", probe]).stdout)
        cached = Path(installed["native"]).resolve()
        if not _is_within(Path(installed["package"]), home) or not _is_within(cached, Path(environment["LOCALAPPDATA"])) or installed["version"] != report["version"] or installed["sha256"] != report["native_sha256"]:
            raise ValueError("Installed SDK resolved an unexpected package or native executable")
        with cached.open("rb"):
            run([python, "-m", "pip", "install", "--force-reinstall", "--no-index", "--no-deps", path])
        run([python, "-m", "pip", "uninstall", "-y", "popu"])
        if not cached.exists():
            raise ValueError("Uninstall removed the persistent runtime cache")
        report["smoke_python"] = sys.version.split()[0]
        report["isolated_install"] = "passed"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", help="Require exact v<Cargo-version> release identity")
    parser.add_argument("--version-only", action="store_true")
    parser.add_argument("--wheel", type=Path)
    parser.add_argument("--smoke", action="store_true")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    native, python = release_version(args.tag)
    if args.version_only:
        print(native)
        return
    if args.wheel is None:
        parser.error("--wheel is required unless --version-only is used")
    path = args.wheel.resolve(strict=True)
    report = inspect_wheel(path, native, python)
    if args.smoke:
        smoke_install(path, report)
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
