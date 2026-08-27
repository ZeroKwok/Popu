from __future__ import annotations

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
class RequestReceipt:
    request_id: str
    state: str
    revision: int
    duplicate: bool = False
    status: str = "accepted"

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "RequestReceipt":
        return cls(str(value["request_id"]), str(value["state"]), int(value["revision"]), bool(value.get("duplicate", False)), str(value.get("status", "accepted")))


NotificationReceipt = RequestReceipt


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
        self._owned: dict[str, subprocess.Popen[str]] = {}
        self._lock = threading.Lock()

    def create(self, request: DialogRequest | Notification | Mapping[str, Any], *, template: str | os.PathLike[str] | None = None) -> RequestReceipt:
        payload = request.as_dict() if hasattr(request, "as_dict") else dict(request)
        path = _write_json(payload)
        command = self._command + ["request", str(path)]
        if template is not None:
            command += ["--template", os.fspath(template)]
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        line = process.stdout.readline() if process.stdout else ""
        path.unlink(missing_ok=True)
        if not line:
            stderr = process.stderr.read() if process.stderr else ""
            process.wait()
            raise PopuError(stderr.strip() or "popu did not accept the request")
        value = json.loads(line)
        # Compatibility with pre-Host test doubles and older executables.
        if "request_id" not in value:
            process.wait()
            receipt = RequestReceipt("", "confirmed", 1, False, str(value.get("status", "accepted")))
            object.__setattr__(receipt, "_terminal", value)
            return receipt
        receipt = RequestReceipt.from_dict(value)
        if payload.get("mode") == "dialog":
            with self._lock:
                self._owned[receipt.request_id] = process
        else:
            process.wait()
        return receipt

    def show(self, request: DialogRequest | Mapping[str, Any]) -> InteractionResult:
        receipt = self.create(request)
        terminal = getattr(receipt, "_terminal", None)
        if terminal is not None:
            return InteractionResult.from_dict(terminal)
        snapshot = self.wait(receipt.request_id)
        response = snapshot.get("response")
        if not isinstance(response, Mapping):
            raise PopuError(f"request ended without a response: {snapshot.get('state')}")
        return InteractionResult.from_dict(response)

    def notify(self, notification: Notification | Mapping[str, Any]) -> NotificationReceipt:
        return self.create(notification)

    def update(self, request_id: str, expected_revision: int, content: Mapping[str, Any]) -> Mapping[str, Any]:
        path = _write_json({"expected_revision": expected_revision, "content": dict(content)})
        try:
            return self._run_json(["update", request_id, str(path)])
        finally:
            path.unlink(missing_ok=True)

    def dismiss(self, request_id: str) -> Mapping[str, Any]:
        return self._run_json(["dismiss", request_id])

    def status(self, request_id: str) -> Mapping[str, Any]:
        return self._run_json(["status", request_id])

    def wait(self, request_id: str) -> Mapping[str, Any]:
        with self._lock:
            process = self._owned.pop(request_id, None)
        if process is None:
            return self._run_json(["wait", request_id])
        stdout, stderr = process.communicate()
        if process.returncode == 12:
            raise PopuError(stderr.strip() or "Popu Host failed")
        lines = [line for line in stdout.splitlines() if line.strip()]
        if not lines:
            raise PopuError(stderr.strip() or "popu returned no terminal result")
        # `request` prints the public Response after its accepted receipt.
        response = json.loads(lines[-1])
        state = {"submitted": "confirmed", "confirmed": "confirmed", "cancelled": "cancelled", "closed": "closed", "timed_out": "expired"}.get(response.get("status"), response.get("status"))
        return {"request_id": request_id, "state": state, "response": response, "exit_code": process.returncode}

    def _run_json(self, args: Sequence[str]) -> Mapping[str, Any]:
        result = subprocess.run(self._command + list(args), capture_output=True, text=True)
        if result.returncode != 0:
            raise PopuError(result.stderr.strip() or f"popu exited with code {result.returncode}")
        try:
            return json.loads(result.stdout.splitlines()[-1])
        except (IndexError, json.JSONDecodeError) as error:
            raise PopuError("popu returned invalid JSON") from error


def _write_json(payload: Mapping[str, Any]) -> Path:
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
