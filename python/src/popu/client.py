from __future__ import annotations

import atexit
import json
import os
import shutil
import subprocess
import tempfile
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping, Sequence

from .models import DialogRequest, InteractionResult, Notification


class PopuError(RuntimeError):
    pass


@dataclass(frozen=True)
class NotificationReceipt:
    status: str


class _LifetimePipe:
    def __init__(self) -> None:
        self.read_fd, self.write_fd = os.pipe()
        os.set_inheritable(self.read_fd, True)

    @property
    def inherited_value(self) -> int:
        if os.name == "nt":
            import msvcrt

            return int(msvcrt.get_osfhandle(self.read_fd))
        return self.read_fd

    def popen_kwargs(self) -> dict[str, Any]:
        if os.name == "nt":
            startupinfo = subprocess.STARTUPINFO()
            startupinfo.lpAttributeList = {"handle_list": [self.inherited_value]}
            return {
                "startupinfo": startupinfo,
                "close_fds": True,
                "creationflags": subprocess.CREATE_NO_WINDOW,
            }
        return {"pass_fds": (self.read_fd,), "close_fds": True}

    def close_reader(self) -> None:
        if self.read_fd >= 0:
            os.close(self.read_fd)
            self.read_fd = -1

    def close(self) -> None:
        self.close_reader()
        if self.write_fd >= 0:
            os.close(self.write_fd)
            self.write_fd = -1


_active_lock = threading.Lock()
_active_notifications: set[tuple[subprocess.Popen[str], _LifetimePipe]] = set()


def _shutdown_notifications() -> None:
    with _active_lock:
        sessions = tuple(_active_notifications)
        _active_notifications.clear()
    for _, lifetime in sessions:
        lifetime.close()


atexit.register(_shutdown_notifications)


class Client:
    def __init__(self, executable: str | os.PathLike[str] | Sequence[str] | None = None) -> None:
        if executable is None:
            resolved = shutil.which("popu")
            if resolved is None:
                raise PopuError("popu executable was not found on PATH")
            self._command = [resolved]
        elif isinstance(executable, (str, os.PathLike)):
            self._command = [os.fspath(executable)]
        else:
            self._command = [os.fspath(item) for item in executable]

    def show(self, request: DialogRequest | Mapping[str, Any]) -> InteractionResult:
        payload = request.as_dict() if isinstance(request, DialogRequest) else dict(request)
        path = _write_request(payload)
        lifetime = _LifetimePipe()
        try:
            command = self._command + [
                "dialog",
                "--request",
                str(path),
                "--wait",
                "--lifetime-handle",
                str(lifetime.inherited_value),
            ]
            process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, **lifetime.popen_kwargs())
            lifetime.close_reader()
            stdout, stderr = process.communicate()
            if process.returncode == 12 or not stdout.strip():
                raise PopuError(stderr.strip() or f"popu exited with code {process.returncode}")
            return InteractionResult.from_dict(json.loads(stdout))
        finally:
            lifetime.close()
            path.unlink(missing_ok=True)

    def notify(self, notification: Notification | Mapping[str, Any]) -> NotificationReceipt:
        payload = notification.as_dict() if isinstance(notification, Notification) else dict(notification)
        path = _write_request(payload)
        lifetime = _LifetimePipe()
        command = self._command + [
            "__notify-child",
            "--request",
            str(path),
            "--lifetime-handle",
            str(lifetime.inherited_value),
        ]
        try:
            process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, **lifetime.popen_kwargs())
            lifetime.close_reader()
            line = process.stdout.readline() if process.stdout else ""
            if not line:
                stderr = process.stderr.read() if process.stderr else ""
                process.wait()
                raise PopuError(stderr.strip() or "popu notification failed before it was shown")
            receipt = json.loads(line)
            if receipt.get("status") != "shown":
                raise PopuError(f"unexpected popu response: {line.strip()}")
            path.unlink(missing_ok=True)
            session = (process, lifetime)
            with _active_lock:
                _active_notifications.add(session)
            threading.Thread(target=_reap_notification, args=(session,), daemon=True).start()
            return NotificationReceipt("shown")
        except Exception:
            lifetime.close()
            path.unlink(missing_ok=True)
            raise


def _reap_notification(session: tuple[subprocess.Popen[str], _LifetimePipe]) -> None:
    process, lifetime = session
    process.wait()
    if process.stdout:
        process.stdout.close()
    if process.stderr:
        process.stderr.close()
    lifetime.close()
    with _active_lock:
        _active_notifications.discard(session)


def _write_request(payload: Mapping[str, Any]) -> Path:
    descriptor, raw_path = tempfile.mkstemp(prefix="popu-", suffix=".json")
    path = Path(raw_path)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(payload, stream, ensure_ascii=False)
        return path
    except Exception:
        path.unlink(missing_ok=True)
        raise


def show(request: DialogRequest | Mapping[str, Any]) -> InteractionResult:
    return Client().show(request)


def notify(notification: Notification | Mapping[str, Any]) -> NotificationReceipt:
    return Client().notify(notification)
