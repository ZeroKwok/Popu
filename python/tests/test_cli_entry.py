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
