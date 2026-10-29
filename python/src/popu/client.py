from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping, Sequence, TextIO

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
        if (
            value.get("status") != "accepted"
            or not isinstance(value.get("request_id"), str)
            or not value["request_id"]
            or not isinstance(value.get("state"), str)
            or type(value.get("revision")) is not int
            or value["revision"] < 1
            or type(value.get("duplicate")) is not bool
        ):
            raise PopuError("popu returned an invalid Host acceptance receipt")
        return cls(
            value["request_id"], value["state"], value["revision"],
            value["duplicate"], value["status"],
        )


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
        if not self._command:
            raise ValueError("executable command must not be empty")

    def create(self, request: DialogRequest | Notification | Mapping[str, Any], *, template: str | os.PathLike[str] | None = None) -> RequestReceipt:
        payload = request.as_dict() if hasattr(request, "as_dict") else dict(request)
        path = _write_json(payload)
        process: subprocess.Popen[str] | None = None
        diagnostics: TextIO | None = None
        try:
            command = self._command + ["request", str(path)]
            if template is not None:
                command += ["--template", os.fspath(template)]
            # A file prevents stderr from filling a pipe while we wait for the
            # receipt. stdout has only one reader; dialog output is drained in
            # the background after acceptance, even if callers never wait().
            diagnostics = tempfile.TemporaryFile(mode="w+", encoding="utf-8")
            process = subprocess.Popen(
                command, stdout=subprocess.PIPE, stderr=diagnostics,
                text=True, encoding="utf-8",
            )
            line = process.stdout.readline() if process.stdout else ""
            if not line:
                process.wait()
                raise PopuError(_diagnostic(diagnostics) or "popu did not accept the request")
            receipt = RequestReceipt.from_dict(_json_object(line))
            if payload.get("mode") == "dialog":
                threading.Thread(
                    target=_reap_request, args=(process, diagnostics), daemon=True,
                ).start()
                process = None
                diagnostics = None
            else:
                process.communicate()
                if process.returncode != 0:
                    raise PopuError(
                        _diagnostic(diagnostics) or f"popu exited with code {process.returncode}"
                    )
            return receipt
        except (OSError, UnicodeError) as error:
            raise PopuError(f"could not submit Popu request: {error}") from error
        finally:
            if process is not None:
                if process.poll() is None:
                    process.terminate()
                if process.stdout is not None:
                    process.stdout.close()
                process.wait()
            if diagnostics is not None:
                diagnostics.close()
            path.unlink(missing_ok=True)

    def show(self, request: DialogRequest | Mapping[str, Any], *, template: str | os.PathLike[str] | None = None) -> InteractionResult:
        receipt = self.create(request, template=template)
        snapshot = self.wait(receipt.request_id)
        response = snapshot.get("response")
        if not isinstance(response, Mapping):
            raise PopuError(f"request ended without a response: {snapshot.get('state')}")
        return InteractionResult.from_dict(response)

    def notify(self, notification: Notification | Mapping[str, Any], *, template: str | os.PathLike[str] | None = None) -> NotificationReceipt:
        return self.create(notification, template=template)

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
        # Always fetch the authoritative snapshot, including revision, content,
        # timestamps and any business exit_code (which may legitimately be 12).
        return self._run_json(["wait", request_id])

    def _run_json(self, args: Sequence[str]) -> Mapping[str, Any]:
        try:
            result = subprocess.run(
                self._command + list(args), capture_output=True, text=True, encoding="utf-8",
            )
        except (OSError, UnicodeError) as error:
            raise PopuError(f"could not run Popu command: {error}") from error
        if result.returncode != 0:
            raise PopuError(result.stderr.strip() or f"popu exited with code {result.returncode}")
        return _json_object(result.stdout)


def _json_object(text: str) -> Mapping[str, Any]:
    try:
        value = json.loads(text)
    except json.JSONDecodeError as error:
        raise PopuError("popu returned invalid JSON") from error
    if not isinstance(value, dict):
        raise PopuError("popu returned a JSON value instead of an object")
    return value


def _diagnostic(stream: TextIO) -> str:
    stream.seek(0)
    return stream.read().strip()


def _reap_request(process: subprocess.Popen[str], diagnostics: TextIO) -> None:
    try:
        process.communicate()
    finally:
        diagnostics.close()


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


def show(request: DialogRequest | Mapping[str, Any], *, template: str | os.PathLike[str] | None = None) -> InteractionResult:
    return Client().show(request, template=template)


def notify(notification: Notification | Mapping[str, Any], *, template: str | os.PathLike[str] | None = None) -> NotificationReceipt:
    return Client().notify(notification, template=template)
