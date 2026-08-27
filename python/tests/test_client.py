from __future__ import annotations

import json
import sys
from pathlib import Path

from popu import Client, DialogRequest, Notification


def _stub(tmp_path: Path) -> Path:
    path = tmp_path / "stub.py"
    path.write_text(
        """
import json, sys, time
request = json.load(open(sys.argv[2], encoding='utf-8'))
if sys.argv[1] == 'request' and request['mode'] == 'dialog':
    print(json.dumps({'version':'1.0','status':'submitted','action':'confirm','values':{}}), flush=True)
elif sys.argv[1] == 'request' and request['mode'] == 'notify':
    print(json.dumps({'status':'shown'}), flush=True)
    time.sleep(0.05)
""",
        encoding="utf-8",
    )
    return path


def test_dialog_round_trip(tmp_path: Path) -> None:
    client = Client([sys.executable, str(_stub(tmp_path))])
    result = client.show(DialogRequest.confirm(title="Continue?"))
    assert result.status == "submitted"
    assert result.action == "confirm"


def test_notification_returns_after_ready(tmp_path: Path) -> None:
    client = Client([sys.executable, str(_stub(tmp_path))])
    receipt = client.notify(Notification.success(title="Done"))
    assert receipt.status == "shown"


def test_builders_emit_protocol_1() -> None:
    request = DialogRequest.confirm(title="Continue?").as_dict()
    notification = Notification.success(title="Done").as_dict()
    assert request["version"] == notification["version"] == "1.0"
    assert request["mode"] == "dialog"
    assert notification["mode"] == "notify"
    json.dumps(request)
