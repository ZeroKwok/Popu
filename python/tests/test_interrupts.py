from __future__ import annotations

import os
import subprocess
import sys
import time
from pathlib import Path

import pytest

from popu import Client, PopuError


def _cli(tmp_path: Path, phase: str) -> list[str]:
    stub = tmp_path / "interrupt_cli.py"
    stub.write_text(r'''
import json, os, sys, time
from pathlib import Path

root, phase, command = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
store = root / "snapshot.json"

def output(value):
    print(json.dumps(value), flush=True)

def stall():
    if os.name == "nt":
        import ctypes
        ctypes.windll.kernel32.SetConsoleCtrlHandler(None, True)
    (root / "waiting").write_text(str(os.getpid()))
    time.sleep(60)

if phase in ("read", "exit", "entry"):
    if phase == "exit":
        output({"state": "queued"})
    stall()
elif command == "request":
    if phase == "notify":
        stall()
    request = json.loads(Path(sys.argv[4]).read_text(encoding="utf-8"))
    snapshot = {"revision": 1, "state": "shown", "updatable": request["updatable"], "content": request["content"]}
    store.write_text(json.dumps(snapshot), encoding="utf-8")
    output({"status": "accepted", "request_id": "demo", "state": "shown", "revision": 1, "duplicate": False})
elif command == "update":
    update = json.loads(Path(sys.argv[5]).read_text(encoding="utf-8"))
    snapshot = json.loads(store.read_text(encoding="utf-8"))
    assert update["expected_revision"] == snapshot["revision"], "stale cleanup revision"
    snapshot.update(content=update["content"], revision=snapshot["revision"] + 1, updatable=update.get("updatable", True))
    store.write_text(json.dumps(snapshot), encoding="utf-8")
    if phase == "update" and snapshot["updatable"]:
        stall()
    output(snapshot)
elif command == "status":
    if phase == "cleanup":
        stall()
    output(json.loads(store.read_text(encoding="utf-8")))
elif command == "wait":
    stall()
''', encoding="utf-8")
    return [sys.executable, str(stub), str(tmp_path), phase]


@pytest.mark.parametrize("phase", ["read", "exit"])
def test_command_deadline_covers_response_and_process_exit(tmp_path: Path, phase: str) -> None:
    client = Client(_cli(tmp_path, phase), timeout=0.5)
    started = time.monotonic()
    with pytest.raises(PopuError, match="timed out"):
        client.status("demo")
    assert time.monotonic() - started < 3


@pytest.mark.parametrize("timeout", [0, -1, float("nan"), float("inf")])
def test_invalid_deadlines_are_rejected(timeout: float) -> None:
    with pytest.raises(ValueError, match="finite and positive"):
        Client(["unused"], timeout=timeout)


@pytest.mark.skipif(os.name != "nt", reason="Windows console Ctrl+C regression")
@pytest.mark.parametrize("phase", ["read", "exit", "notify", "sleep", "update", "wait", "cleanup", "entry", "runner", "module"])
def test_real_ctrl_c_interrupts_sdk_and_progress_demo(tmp_path: Path, phase: str) -> None:
    command = _cli(tmp_path, phase)
    repository = Path(__file__).resolve().parents[2]
    controller = tmp_path / "controller.py"
    controller.write_text('''
import json, runpy, sys
from pathlib import Path
from popu import Client
# This private console must not inherit a launching shell's SIGINT-ignore state.
if sys.platform == "win32":
    import ctypes, signal
    ctypes.windll.kernel32.SetConsoleCtrlHandler(None, False)
    signal.signal(signal.SIGINT, signal.default_int_handler)
command, phase, root, repository = json.loads(sys.argv[1]), sys.argv[2], Path(sys.argv[3]), Path(sys.argv[4])
from popu.demo import progress
progress.Client = lambda executable=None, **kwargs: Client(command, **kwargs)
original_sleep = progress.time.sleep
def demo_sleep(seconds):
    if phase in ("sleep", "cleanup", "runner", "module"):
        (root / "waiting").write_text("0")
        original_sleep(60)
    else:
        original_sleep(0.01)
progress.time.sleep = demo_sleep
if phase == "module":
    sys.argv = ["popu.demo", "progress", "--wait"]
    runpy.run_module("popu.demo", run_name="__main__")
elif phase == "runner":
    sys.path.insert(0, str(repository / "scripts"))
    from dev import python_demo
    python_demo(["--", "--executable", command[0], "--wait"])
elif phase == "entry":
    from popu import _cli
    _cli.executable_path = lambda: Path(command[0])
    sys.argv = ["popu", *command[1:], "status"]
    raise SystemExit(_cli.main())
elif phase in ("read", "exit"):
    try:
        Client(command).status("demo")
    except KeyboardInterrupt:
        raise SystemExit(130)
else:
    raise SystemExit(progress.main(["--wait"]))
''', encoding="utf-8")
    import json

    environment = {**os.environ, "PYTHONPATH": str(Path(__file__).parents[1] / "src"), "PYTHONIOENCODING": "utf-8"}
    startup = subprocess.STARTUPINFO()
    startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startup.wShowWindow = 0
    log = tmp_path / "console.log"
    with log.open("wb") as output:
        target = subprocess.Popen(
            [sys.executable, str(controller), json.dumps(command), phase, str(tmp_path), str(repository)],
            env=environment, stdout=output, stderr=output,
            creationflags=subprocess.CREATE_NEW_CONSOLE, startupinfo=startup,
        )
        try:
            deadline = time.monotonic() + 10
            while not (tmp_path / "waiting").exists() and target.poll() is None and time.monotonic() < deadline:
                time.sleep(0.01)
            assert (tmp_path / "waiting").exists(), log.read_text(encoding="utf-8")
            # Attach only the sender to this test's private hidden console.
            # CTRL_C_EVENT never reaches the user's console or running Host.
            sender = r'''
import ctypes, sys, time
kernel = ctypes.WinDLL("kernel32", use_last_error=True)
kernel.FreeConsole()
if not kernel.AttachConsole(int(sys.argv[1])):
    raise ctypes.WinError(ctypes.get_last_error())
kernel.SetConsoleCtrlHandler(None, True)
if not kernel.GenerateConsoleCtrlEvent(0, 0):
    raise ctypes.WinError(ctypes.get_last_error())
time.sleep(0.1)
kernel.FreeConsole()
'''
            subprocess.run([sys.executable, "-c", sender, str(target.pid)], check=True, timeout=5)
            assert target.wait(timeout=4) == 130, log.read_text(encoding="utf-8")
        finally:
            if target.poll() is None:
                target.kill()
            target.wait()
    if phase not in ("read", "exit", "notify", "cleanup", "entry"):
        snapshot = json.loads((tmp_path / "snapshot.json").read_text(encoding="utf-8"))
        assert snapshot["updatable"] is False
        assert snapshot["content"]["title"] == ("Workspace backup complete" if phase == "wait" else "Backup demo stopped")
