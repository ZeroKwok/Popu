from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from popu import PopuError, _cli


@pytest.mark.parametrize("exit_code", [0, 10, 11, 12, 47])
def test_cli_forwards_arguments_streams_and_exit_codes(tmp_path: Path, exit_code: int) -> None:
    native = tmp_path / "native mock.py"
    native.write_text("import json,sys\nprint(json.dumps({'args':sys.argv[2:], 'stdin':sys.stdin.read()},ensure_ascii=False),flush=True)\nprint('diagnostic',file=sys.stderr)\nsys.exit(int(sys.argv[1]))\n", encoding="utf-8")
    launcher = "from pathlib import Path; import sys; from popu import _cli; _cli.executable_path=lambda:Path(sys.executable); sys.argv=['popu',*sys.argv[1:]]; sys.exit(_cli.main())"
    environment = {**os.environ, "PYTHONPATH": str(Path(__file__).parents[1] / "src"), "PYTHONIOENCODING": "utf-8"}
    result = subprocess.run([sys.executable, "-c", launcher, str(native), str(exit_code), "with spaces", "中文"], input="payload 中文", capture_output=True, text=True, encoding="utf-8", env=environment, timeout=10)
    assert result.returncode == exit_code
    assert json.loads(result.stdout) == {"args": ["with spaces", "中文"], "stdin": "payload 中文"}
    assert result.stderr.strip() == "diagnostic"


def test_cli_resolution_errors_go_to_stderr(monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture) -> None:
    def missing() -> Path:
        raise PopuError("missing payload")
    monkeypatch.setattr(_cli, "executable_path", missing)
    assert _cli.main() == 12
    captured = capsys.readouterr()
    assert captured.out == ""
    assert "missing payload" in captured.err


@pytest.mark.skipif(os.name != "nt", reason="Windows console handle regression")
def test_cli_preserves_output_in_a_real_console(tmp_path: Path) -> None:
    # Pipe capture cannot reproduce invalid console handles under CREATE_NO_WINDOW.
    # This private hidden console keeps the regression away from the user's terminal.
    native = tmp_path / "console_native.py"
    native.write_text(
        "import sys\nprint('native-stdout', flush=True)\n"
        "print('native-stderr', file=sys.stderr, flush=True)\nsys.exit(47)\n",
        encoding="utf-8",
    )
    launcher = tmp_path / "launcher.py"
    launcher.write_text(
        "from pathlib import Path\nimport sys\nfrom popu import _cli\n"
        "_cli.executable_path = lambda: Path(sys.executable)\n"
        "sys.argv = ['popu', sys.argv[1]]\nraise SystemExit(_cli.main())\n",
        encoding="utf-8",
    )
    controller = tmp_path / "console_controller.py"
    controller.write_text(r'''
import ctypes, json, subprocess, sys
from ctypes import wintypes
from pathlib import Path

class Coord(ctypes.Structure):
    _fields_ = [("X", wintypes.SHORT), ("Y", wintypes.SHORT)]

class Rect(ctypes.Structure):
    _fields_ = [("left", wintypes.SHORT), ("top", wintypes.SHORT),
               ("right", wintypes.SHORT), ("bottom", wintypes.SHORT)]

class BufferInfo(ctypes.Structure):
    _fields_ = [("size", Coord), ("cursor", Coord), ("attributes", wintypes.WORD),
               ("window", Rect), ("maximum", Coord)]

kernel = ctypes.WinDLL("kernel32", use_last_error=True)
kernel.GetStdHandle.argtypes = [wintypes.DWORD]
kernel.GetStdHandle.restype = wintypes.HANDLE
kernel.GetConsoleScreenBufferInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(BufferInfo)]
kernel.ReadConsoleOutputCharacterW.argtypes = [wintypes.HANDLE, wintypes.LPWSTR,
    wintypes.DWORD, Coord, ctypes.POINTER(wintypes.DWORD)]
result = subprocess.run([sys.executable, sys.argv[1], sys.argv[2]], timeout=10)
handle = kernel.GetStdHandle(-11)
info = BufferInfo()
if not kernel.GetConsoleScreenBufferInfo(handle, ctypes.byref(info)):
    raise ctypes.WinError(ctypes.get_last_error())
length = info.size.X * (info.cursor.Y + 1)
buffer = ctypes.create_unicode_buffer(length)
read = wintypes.DWORD()
if not kernel.ReadConsoleOutputCharacterW(handle, buffer, length, Coord(0, 0), ctypes.byref(read)):
    raise ctypes.WinError(ctypes.get_last_error())
Path(sys.argv[3]).write_text(json.dumps({"exit": result.returncode, "screen": buffer[:read.value]}), encoding="utf-8")
''', encoding="utf-8")
    environment = {**os.environ, "PYTHONPATH": str(Path(__file__).parents[1] / "src"), "PYTHONIOENCODING": "utf-8"}
    startup = subprocess.STARTUPINFO()
    startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startup.wShowWindow = 0
    report = tmp_path / "console.json"
    subprocess.run(
        [sys.executable, str(controller), str(launcher), str(native), str(report)],
        env=environment, creationflags=subprocess.CREATE_NEW_CONSOLE, startupinfo=startup,
        check=True, timeout=15,
    )
    result = json.loads(report.read_text(encoding="utf-8"))
    assert result["exit"] == 47, result
    assert "native-stdout" in result["screen"], result
    assert "native-stderr" in result["screen"], result
