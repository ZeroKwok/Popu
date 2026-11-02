"""Simulate a backup to demonstrate live notifications; no files are copied.

Run after installing the SDK and building the matching Popu executable:
    python python/examples/backup_progress.py --executable src-tauri/target/debug/popu.exe

Close the progress card: subsequent reports stay silent, and the final report
requests the single allowed reshow. Leave it open to see in-place updates, or
use --display-seconds 5 to see automatic expiry followed by the next report.
"""
from __future__ import annotations

import argparse
import time
from pathlib import Path

from popu import Client, Notification, PopuError


def content(percent: int, elapsed: float, *, finished: bool = False) -> dict:
    copied = 2000 * percent // 100
    return {
        "severity": "success" if finished else "info",
        "title": "备份完成" if finished else "正在备份 Documents",
        "blocks": [
            {"type": "paragraph", "text": "演示备份已完成，所有文件校验通过。" if finished else "正在复制并校验文件；后台任务继续运行。"},
            {"type": "properties", "items": [
                {"label": "文件", "text": f"{copied:,} / 2,000"},
                {"label": "数据量", "text": f"{2.4 * percent / 100:.2f} / 2.40 GB"},
                {"label": "耗时", "text": f"{elapsed:.0f} 秒"},
                {"label": "目标", "text": "Daily backup（模拟）"},
            ]},
            {"type": "progress", "title": "备份进度", "value": percent},
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--executable", type=Path, help="Path to the matching popu executable; defaults to PATH")
    parser.add_argument("--interval", type=float, default=15, help="Seconds between reports (default: 3)")
    parser.add_argument("--steps", type=int, default=6, help="Number of report intervals (default: 12)")
    parser.add_argument("--display-seconds", type=float, default=20, help="Initial display duration (default: 20)")
    parser.add_argument("--final-seconds", type=float, default=10, help="Final display duration (default: 10)")
    parser.add_argument("--wait", action="store_true", help="Wait for the final presentation to end")
    args = parser.parse_args()
    if args.interval <= 0 or args.steps < 1 or not 0.5 <= args.display_seconds <= 86400 or not 0.5 <= args.final_seconds <= 86400:
        parser.error("interval must be positive, steps >= 1, and display durations between 0.5 and 86400 seconds")

    client = Client(args.executable)
    initial = content(0, 0)
    notification = Notification.success(
        title=initial["title"], blocks=initial["blocks"], updatable=True,
        duration_ms=round(args.display_seconds * 1000), width=440, height=360,
    ).as_dict()
    notification["content"] = initial
    receipt = None
    finished = False
    try:
        receipt = client.notify(notification)
        revision = receipt.revision
        started = time.monotonic()
        print(f"通知 ID: {receipt.request_id}", flush=True)
        print("可点击 × 收起进度；Ctrl+C 停止演示；结束时申请最后显示一次。", flush=True)
        for step in range(1, args.steps + 1):
            time.sleep(args.interval)
            percent = step * 100 // args.steps
            is_final = step == args.steps
            snapshot = client.update(
                receipt.request_id, revision, content(percent, time.monotonic() - started, finished=is_final),
                **({"updatable": False, "reshow": True, "duration_ms": round(args.final_seconds * 1000)} if is_final else {}),
            )
            revision = snapshot["revision"]
            finished = is_final
            print(f"{percent:3d}% · state={snapshot['state']} · revision={revision} · reshow_suppressed={snapshot.get('reshow_suppressed', False)}", flush=True)
        if args.wait:
            result = client.wait(receipt.request_id)
            print(f"已归档：{result['state']}；可在托盘历史查看最终摘要。", flush=True)
    except KeyboardInterrupt:
        print("\n备份演示已停止。", flush=True)
        if receipt is not None and not finished:
            _stop_notification(args.executable, receipt.request_id)
        return 130
    except PopuError as error:
        print(f"通知已无法更新：{error}", flush=True)
        return 1
    return 0


def _stop_notification(executable: Path | None, request_id: str) -> None:
    # An interrupted update may already have reached the Host. Fetch its latest
    # revision, and bound both cleanup commands so shutdown cannot hang.
    try:
        client = Client(executable, timeout=1)
        snapshot = client.status(request_id)
        if snapshot.get("updatable"):
            client.update(request_id, snapshot["revision"], {
                "severity": "warning", "title": "备份演示已停止",
                "blocks": [{"type": "paragraph", "text": "演示被中断，没有复制实际文件。"}],
            }, updatable=False, duration_ms=5000)
    except (PopuError, KeyboardInterrupt) as error:
        print(f"通知收尾已跳过：{error or '再次中断'}", flush=True)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except PopuError as error:
        print(f"无法启动演示：{error}", flush=True)
        raise SystemExit(1)
