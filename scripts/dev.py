"""Run development examples without embedding control flow in shell recipes."""
from __future__ import annotations

import argparse
from pathlib import Path
import runpy
import subprocess
import sys
from typing import Sequence

from project_paths import ROOT, native_executable, require_executable, require_windows

FIXTURES = ROOT / "protocol/examples"


def fixture_path(value: str) -> Path:
    path = Path(value)
    if not path.is_absolute() and not path.is_file():
        path = FIXTURES / path
    return path.resolve(strict=True)


def run_fixture(command: Sequence[str], request: Path) -> int:
    arguments = [*command, "request", str(request)]
    sidecar = Path(str(request).removesuffix(".json") + ".template")
    if sidecar.is_file():
        # Preserve the old contract: first line, relative to repository cwd.
        lines = sidecar.read_text(encoding="utf-8").splitlines()
        template = lines[0] if lines else ""
        if not template:
            raise ValueError(f"Empty template sidecar: {sidecar}")
        arguments.extend(["--template", template])
    return subprocess.run(arguments, cwd=ROOT).returncode


def run_fixtures(command: Sequence[str], prefix: str) -> int:
    requests = sorted(path for path in FIXTURES.glob("*.json") if path.name.startswith(prefix))
    if not requests:
        raise ValueError(f"No protocol examples found for prefix: {prefix}")
    for request in requests:
        print(f"\n==> {request.name}", flush=True)
        code = run_fixture(command, request)
        if code not in (0, 10, 11):
            return code
    return 0


def python_demo(arguments: list[str]) -> None:
    demo = ROOT / "python/examples/progress.py"
    if arguments[:1] == ["--"]:
        arguments = arguments[1:]
    parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    parser.add_argument("--executable")
    explicit, _ = parser.parse_known_args(arguments)
    if not explicit.executable and not any(value in ("-h", "--help") for value in arguments):
        arguments = ["--executable", str(require_executable(native_executable())), *arguments]
    sys.argv = [str(demo), *arguments]
    # Keep the existing demo's Ctrl+C cleanup in this process.
    runpy.run_path(str(demo), run_name="__main__")


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    fixture = commands.add_parser("fixture", help="Run one JSON request and its optional template sidecar")
    fixture.add_argument("request")
    fixtures = commands.add_parser("fixtures", help="Run requests with a literal filename prefix")
    fixtures.add_argument("prefix", nargs="?", default="")
    for command in (fixture, fixtures):
        command.add_argument("--profile", choices=("debug", "release"), default="debug")
        command.add_argument("--executable", type=Path)
    demo = commands.add_parser("python", help="Run the Python progress example in this interpreter")
    demo.add_argument("arguments", nargs=argparse.REMAINDER)
    commands.add_parser("native-smoke", help="Run the current Windows/WebView2 smoke script")
    args = parser.parse_args(argv)
    try:
        if args.command == "python":
            python_demo(args.arguments)
            return 0
        if args.command == "native-smoke":
            require_windows("Native visual smoke testing")
            executable = require_executable(native_executable())
            return subprocess.run(["node", str(ROOT / "scripts/native-visual-smoke.mjs"), str(executable)], cwd=ROOT).returncode
        executable = require_executable(args.executable.resolve() if args.executable else native_executable(args.profile))
        if args.command == "fixture":
            return run_fixture([str(executable)], fixture_path(args.request))
        return run_fixtures([str(executable)], args.prefix)
    except KeyboardInterrupt:
        return 130
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        diagnostic = error.stderr.strip() if isinstance(error, subprocess.CalledProcessError) and error.stderr else str(error)
        print(f"Popu development error: {diagnostic}", file=sys.stderr)
        return 12


if __name__ == "__main__":
    raise SystemExit(main())
