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
    assert "Simulated progress demo" in output.out
    assert "no backup files will be read, copied, or modified" in output.out
    assert "bundled runtime unavailable" in output.err
    assert (output.out + output.err).isascii()


@pytest.mark.parametrize("phase", ["complete", "error", "interrupt", "cleanup-error", "cleanup-interrupt"])
def test_demo_console_is_english_during_updates_wait_and_cleanup(monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture, phase: str) -> None:
    from popu import RequestReceipt

    class DemoClient:
        def __init__(self, *args, **kwargs):
            pass

        def notify(self, notification):
            return RequestReceipt("demo", "shown", 1)

        def update(self, request_id, revision, content, **kwargs):
            if phase == "error":
                raise PopuError("update failed")
            if phase != "complete":
                raise KeyboardInterrupt
            return {"revision": revision + 1, "state": "shown"}

        def status(self, request_id):
            if phase == "cleanup-error":
                raise PopuError("cleanup failed")
            if phase == "cleanup-interrupt":
                raise KeyboardInterrupt
            return {"revision": 1, "updatable": True}

        def wait(self, request_id):
            return {"state": "completed"}

    monkeypatch.setattr(progress, "Client", DemoClient)
    monkeypatch.setattr(progress.time, "sleep", lambda _: None)
    result = main(["progress", "--steps", "2", "--wait"])
    assert result == (0 if phase == "complete" else 1 if phase == "error" else 130)
    output = capsys.readouterr()
    assert (output.out + output.err).isascii()
    assert "Notification ID: demo" in output.out
    if phase == "complete":
        assert "100% | state=shown" in output.out
        assert "Archived: completed" in output.out
    elif phase == "error":
        assert "Could not run the simulated progress demo: update failed" in output.err
    else:
        assert "Simulated progress demo stopped." in output.out
        assert "Notification cleanup skipped:" in output.out


def test_demo_dispatches_registered_examples_with_their_own_options(monkeypatch: pytest.MonkeyPatch) -> None:
    from types import SimpleNamespace
    from popu.demo.__main__ import DEMOS

    received = []
    def run(arguments):
        received.extend(arguments)
        return 7

    monkeypatch.setitem(DEMOS, "another", SimpleNamespace(__doc__="Another SDK example.", main=run))
    assert main(["another", "--own-option", "value"]) == 7
    assert received == ["--own-option", "value"]
