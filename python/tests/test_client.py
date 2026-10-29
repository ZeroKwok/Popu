from __future__ import annotations

import json
import sys
import time
from pathlib import Path

import pytest

from popu import Client, DialogRequest, Notification, PopuError, Screen, ScreenArea, VerticalAlign


def _stub(tmp_path: Path, scenario: str = "") -> Path:
    """Exercise the public CLI wire format, without opening desktop windows."""
    path = tmp_path / "stub.py"
    path.write_text(
        f"SCENARIO = {scenario!r}\n" + r'''
import json, sys, time
from pathlib import Path

root = Path(__file__).parent
command = sys.argv[1]
request_id = "7fd95dfe-f802-499a-aa85-719bd0cafcda"
store = root / "snapshot.json"

def save(snapshot):
    temporary = root / "snapshot.tmp"
    temporary.write_text(json.dumps(snapshot, ensure_ascii=False), encoding="utf-8")
    temporary.replace(store)

def output(value):
    sys.stdout.buffer.write((json.dumps(value, ensure_ascii=False) + "\n").encode("utf-8"))
    sys.stdout.buffer.flush()

def fail(message):
    sys.stderr.buffer.write(message.encode("utf-8"))
    sys.exit(12)

if command == "request":
    source = Path(sys.argv[2])
    request = json.loads(source.read_text(encoding="utf-8"))
    (root / "input.json").write_text(json.dumps({
        "request": request, "source": str(source), "args": sys.argv[3:],
    }), encoding="utf-8")
    if SCENARIO == "reject":
        fail("请求校验失败")
    if SCENARIO == "legacy":
        output({"status": "submitted", "action": "confirm", "values": {}})
        sys.exit(0)
    if SCENARIO == "invalid_json":
        print("not JSON", flush=True)
        sys.exit(0)
    if SCENARIO == "no_output":
        sys.exit(0)
    if SCENARIO == "stderr_burst":
        sys.stderr.buffer.write(b"x" * (256 * 1024))
        sys.stderr.buffer.flush()
    snapshot = {
        "request_id": request_id, "state": "queued", "revision": 1,
        "mode": request["mode"], "requires_read": request.get("requires_read", False),
        "content": request["content"], "created_at": {"secs_since_epoch": 1, "nanos_since_epoch": 0},
    }
    receipt = {
        "status": "accepted", "request_id": request_id,
        "state": "queued", "revision": 1, "duplicate": SCENARIO == "duplicate",
    }
    if request["mode"] == "dialog" and SCENARIO != "pending":
        status = SCENARIO if SCENARIO in ("cancelled", "closed", "timed_out") else "submitted"
        response = {"version": "1.0", "status": status, "action": "confirm", "values": {"name": "中文结果"}}
        snapshot.update(
            state={"submitted": "confirmed", "timed_out": "expired"}.get(status, status),
            response=response, revision=3, ended_at={"secs_since_epoch": 2, "nanos_since_epoch": 0},
            exit_code={"cancelled": 10, "closed": 10, "timed_out": 11}.get(status, 12),
        )
        save(snapshot)
        # Write both lines together to catch stdout buffering regressions.
        sys.stdout.buffer.write((json.dumps(receipt) + "\n" + json.dumps(response, ensure_ascii=False) + "\n").encode("utf-8"))
        sys.stdout.buffer.flush()
        sys.exit(snapshot["exit_code"])
    save(snapshot)
    output(receipt)
    if SCENARIO == "pending":
        deadline = time.monotonic() + 5
        while not (root / "release").exists() and time.monotonic() < deadline:
            time.sleep(0.01)
        (root / "reaped").touch()
        output({"version": "1.0", "status": "cancelled", "action": None, "values": {}})
    if SCENARIO == "notify_exit_error":
        fail("提交进程失败")
else:
    if SCENARIO == "management_json":
        output([])
        sys.exit(0)
    snapshot = json.loads(store.read_text(encoding="utf-8"))
    if sys.argv[2] != request_id:
        fail('Host RPC failed: {"code":"not_found"}')
    if command == "update":
        update = json.loads(Path(sys.argv[3]).read_text(encoding="utf-8"))
        (root / "update-path.txt").write_text(sys.argv[3], encoding="utf-8")
        if update["expected_revision"] != snapshot["revision"]:
            fail('Host RPC failed: {"code":"revision_conflict","current_revision":%d}' % snapshot["revision"])
        snapshot.update(content=update["content"], revision=snapshot["revision"] + 1)
        save(snapshot)
    elif command == "dismiss":
        snapshot.update(state="cancelled", exit_code=10)
        save(snapshot)
        (root / "release").touch()
    elif command == "wait" and snapshot["state"] == "queued":
        snapshot.update(state="expired", exit_code=11)
        save(snapshot)
        (root / "release").touch()
    output(snapshot)
''',
        encoding="utf-8",
    )
    return path


def _client(tmp_path: Path, scenario: str = "") -> Client:
    return Client([sys.executable, str(_stub(tmp_path, scenario))])


def test_dialog_round_trip_with_unicode_and_business_exit_code_12(tmp_path: Path) -> None:
    client = _client(tmp_path)
    result = client.show(DialogRequest.confirm(title="继续？"))
    assert result.status == "submitted"
    assert result.action == "confirm"
    assert result.values == {"name": "中文结果"}


@pytest.mark.parametrize("status", ["cancelled", "closed", "timed_out"])
def test_dialog_nonzero_outcomes_are_business_results(tmp_path: Path, status: str) -> None:
    result = _client(tmp_path, status).show(DialogRequest.confirm(title="Continue?"))
    assert result.status == status


def test_wait_returns_the_same_full_snapshot_for_any_client(tmp_path: Path) -> None:
    executable = [sys.executable, str(_stub(tmp_path))]
    client = Client(executable)
    receipt = client.create(DialogRequest.confirm(title="Continue?"))
    snapshot = client.wait(receipt.request_id)
    assert snapshot == Client(executable).wait(receipt.request_id)
    assert snapshot["revision"] == 3
    assert snapshot["exit_code"] == 12
    assert snapshot["mode"] == "dialog"
    assert snapshot["content"]["title"] == "Continue?"
    assert "created_at" in snapshot and "ended_at" in snapshot


def test_notification_returns_acceptance_receipt(tmp_path: Path) -> None:
    client = _client(tmp_path)
    receipt = client.notify(Notification.success(title="Done"))
    assert receipt.status == "accepted"
    assert receipt.state == "queued"
    assert receipt.revision == 1
    assert receipt.request_id
    assert receipt.duplicate is False
    assert client.status(receipt.request_id)["state"] == "queued"


def test_duplicate_receipt_preserves_host_identity(tmp_path: Path) -> None:
    receipt = _client(tmp_path, "duplicate").notify(
        Notification.success(title="Done", coalesce_key="backup")
    )
    assert receipt.duplicate is True
    assert receipt.request_id == "7fd95dfe-f802-499a-aa85-719bd0cafcda"


def test_update_conflicts_and_dismissal(tmp_path: Path) -> None:
    client = _client(tmp_path)
    receipt = client.notify(Notification.success(title="备份中"))
    content = {"severity": "success", "title": "备份完成", "blocks": []}
    updated = client.update(receipt.request_id, receipt.revision, content)
    assert updated["revision"] == 2
    assert updated["content"] == content
    assert client.status(receipt.request_id)["content"] == content
    with pytest.raises(PopuError, match="revision_conflict"):
        client.update(receipt.request_id, receipt.revision, content)
    assert client.dismiss(receipt.request_id)["state"] == "cancelled"
    assert client.wait(receipt.request_id)["state"] == "cancelled"
    update_path = Path((tmp_path / "update-path.txt").read_text(encoding="utf-8"))
    assert not update_path.exists()


def test_pending_dialog_is_accepted_without_waiting_and_reaped(tmp_path: Path) -> None:
    client = _client(tmp_path, "pending")
    receipt = client.create(DialogRequest.confirm(title="Pending"))
    assert not (tmp_path / "reaped").exists()
    assert client.status(receipt.request_id)["state"] == "queued"
    client.dismiss(receipt.request_id)
    deadline = time.monotonic() + 3
    while not (tmp_path / "reaped").exists() and time.monotonic() < deadline:
        time.sleep(0.01)
    assert (tmp_path / "reaped").exists()


@pytest.mark.parametrize("method", ["show", "notify"])
def test_template_option_and_request_file_cleanup(tmp_path: Path, method: str) -> None:
    client = _client(tmp_path)
    request = DialogRequest.confirm(title="Confirm") if method == "show" else Notification.success(title="Done")
    template = tmp_path / "custom template"
    getattr(client, method)(request, template=template)
    submitted = json.loads((tmp_path / "input.json").read_text(encoding="utf-8"))
    assert submitted["args"] == ["--template", str(template)]
    assert not Path(submitted["source"]).exists()


@pytest.mark.parametrize("scenario, message", [
    ("reject", "请求校验失败"),
    ("legacy", "invalid Host acceptance receipt"),
    ("invalid_json", "invalid JSON"),
    ("no_output", "did not accept"),
    ("notify_exit_error", "提交进程失败"),
])
def test_invalid_submissions_raise_and_clean_up(tmp_path: Path, scenario: str, message: str) -> None:
    with pytest.raises(PopuError, match=message):
        _client(tmp_path, scenario).notify(Notification.success(title="Done"))
    submitted = json.loads((tmp_path / "input.json").read_text(encoding="utf-8"))
    assert not Path(submitted["source"]).exists()


def test_stderr_does_not_block_acceptance(tmp_path: Path) -> None:
    assert _client(tmp_path, "stderr_burst").notify(Notification.success(title="Done")).request_id


def test_management_errors_are_reported(tmp_path: Path) -> None:
    client = _client(tmp_path)
    client.notify(Notification.success(title="Done"))
    with pytest.raises(PopuError, match="not_found"):
        client.status("missing")


def test_management_json_must_be_an_object(tmp_path: Path) -> None:
    client = _client(tmp_path, "management_json")
    receipt = client.notify(Notification.success(title="Done"))
    with pytest.raises(PopuError, match="instead of an object"):
        client.status(receipt.request_id)


def test_missing_executable_raises_sdk_error_and_cleans_up(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("tempfile.tempdir", str(tmp_path))
    client = Client(tmp_path / "missing-popu")
    with pytest.raises(PopuError, match="could not submit"):
        client.notify(Notification.success(title="Done"))
    assert not list(tmp_path.glob("popu-*.json"))
    with pytest.raises(PopuError, match="could not run"):
        client.status("missing")


def test_empty_executable_is_rejected() -> None:
    with pytest.raises(ValueError, match="must not be empty"):
        Client([])


def test_builders_emit_current_protocol() -> None:
    request = DialogRequest.confirm(title="Continue?").as_dict()
    notification = Notification.success(title="Done").as_dict()
    assert request["version"] == notification["version"] == "1.0"
    assert request["mode"] == "dialog"
    assert notification["mode"] == "notify"
    assert notification["requires_read"] is False
    assert "requires_read" not in request
    assert notification["window"] == {
        "always_on_top": True, "screen": "primary", "area": "work_area",
    }
    assert request["content"]["vertical_align"] == "center"
    json.dumps(request)


def test_notification_can_require_reading() -> None:
    notification = Notification.success(title="Review result", requires_read=True).as_dict()
    assert notification["requires_read"] is True


def test_builder_supports_percentage_screen_dimensions() -> None:
    request = DialogRequest.confirm(
        title="Fullscreen", screen=Screen.PRIMARY, area=ScreenArea.SCREEN,
        width="100%", height="80%", always_on_top=False,
    ).as_dict()
    assert request["window"] == {
        "always_on_top": False, "screen": "primary", "area": "screen",
        "width": "100%", "height": "80%",
    }


def test_builders_support_coalescing_and_alignment() -> None:
    for builder in (DialogRequest.confirm, Notification.success):
        request = builder(title="Update", coalesce_key="job:backup", vertical_align=VerticalAlign.START).as_dict()
        assert request["coalesce_key"] == "job:backup"
        assert request["content"]["vertical_align"] == "start"
        assert "coalesce_key" not in builder(title="Default").as_dict()


def test_notification_rejects_full_screen_area() -> None:
    with pytest.raises(ValueError, match="work_area"):
        Notification.success(title="Done", area=ScreenArea.SCREEN)


def test_notification_has_no_non_topmost_option() -> None:
    with pytest.raises(TypeError, match="always_on_top"):
        Notification.success(title="Done", always_on_top=False)
