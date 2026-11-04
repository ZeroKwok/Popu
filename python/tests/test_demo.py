from __future__ import annotations

import pytest

from popu import PopuError
from popu.demo import progress
from popu.demo.__main__ import main


@pytest.mark.parametrize("arguments", [["--help"], ["progress", "--help"]])
def test_demo_help_explains_simulation_without_resolving_a_runtime(monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture, arguments: list[str]) -> None:
    monkeypatch.setattr(progress, "Client", lambda *args: pytest.fail("help must not start a client"))
    with pytest.raises(SystemExit) as result:
        main(arguments)
    assert result.value.code == 0
    assert "simulat" in capsys.readouterr().out.lower()


@pytest.mark.parametrize("arguments", [[], ["backup"], ["progress", "--interval", "nan"], ["progress", "--interval", "inf"], ["progress", "--steps", "0"], ["progress", "--unknown"]])
def test_invalid_demo_arguments_fail_before_submitting_any_notification(monkeypatch: pytest.MonkeyPatch, arguments: list[str]) -> None:
    monkeypatch.setattr(progress, "Client", lambda *args: pytest.fail("invalid options must not start a client"))
    with pytest.raises(SystemExit) as result:
        main(arguments)
    assert result.value.code == 2


def test_demo_startup_errors_are_reported_without_a_traceback(monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture) -> None:
    def unavailable(*args):
        raise PopuError("bundled runtime unavailable")
    monkeypatch.setattr(progress, "Client", unavailable)
    assert main(["progress"]) == 1
    output = capsys.readouterr()
    assert "模拟进度演示" in output.out
    assert "不会读取、复制或修改你的备份文件" in output.out
    assert "bundled runtime unavailable" in output.err
