"""Demonstrate simulated progress through the Popu Python SDK.

The mock backup scenario only generates progress and statistics. It never
reads, copies, modifies, or verifies your backup files.

Run after installing the Windows wheel:
    python -m popu.demo progress --wait
Quick demonstration:
    python -m popu.demo progress --interval 1 --steps 5 --wait

Close the progress card: subsequent reports stay silent, and the final report
requests the single allowed reshow. Leave it open to see in-place updates, or
use --display-seconds 5 to see automatic expiry followed by the next report.
"""
from __future__ import annotations

import argparse
import math
import sys
import time
from pathlib import Path
from typing import Sequence

from popu import Client, Notification, PopuError


def content(percent: int, elapsed: float, *, finished: bool = False) -> dict:
    simulated_files = 2000 * percent // 100
    return {
        "severity": "success" if finished else "info",
        "title": "模拟进度演示完成" if finished else "模拟进度演示 · 进行中",
        "blocks": [
            {"type": "paragraph", "text": "模拟进度演示已完成。未执行真实备份，未对你的备份文件做任何操作。" if finished else "以模拟备份场景展示进度，文件数量和数据量均为模拟。不会读取、复制或修改你的备份文件。"},
            {"type": "properties", "items": [
                {"label": "文件（模拟）", "text": f"{simulated_files:,} / 2,000"},
                {"label": "数据量（模拟）", "text": f"{2.4 * percent / 100:.2f} / 2.40 GB"},
                {"label": "演示耗时", "text": f"{elapsed:.0f} 秒"},
                {"label": "场景", "text": "模拟备份 · 虚拟目标"},
            ]},
            {"type": "progress", "title": "模拟进度", "value": percent},
        ],
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m popu.demo progress", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--executable", type=Path, help="Path to a local popu executable; defaults to the bundled runtime")
    parser.add_argument("--interval", type=float, default=3, help="Seconds between reports (default: 3)")
    parser.add_argument("--steps", type=int, default=12, help="Number of report intervals (default: 12)")
    parser.add_argument("--display-seconds", type=float, default=20, help="Initial display duration (default: 20)")
    parser.add_argument("--final-seconds", type=float, default=10, help="Final display duration (default: 10)")
    parser.add_argument("--wait", action="store_true", help="Wait for the final presentation to end")
    args = parser.parse_args(argv)
    if not math.isfinite(args.interval) or args.interval <= 0 or args.steps < 1 or not 0.5 <= args.display_seconds <= 86400 or not 0.5 <= args.final_seconds <= 86400:
        parser.error("interval must be finite and positive, steps >= 1, and display durations between 0.5 and 86400 seconds")

    receipt = None
    finished = False
    try:
        print("模拟进度演示（模拟备份场景）：不会读取、复制或修改你的备份文件。", flush=True)
        client = Client(args.executable)
        initial = content(0, 0)
        notification = Notification.success(
            title=initial["title"], blocks=initial["blocks"], updatable=True,
            duration_ms=round(args.display_seconds * 1000), width=440, height=360,
        ).as_dict()
        notification["content"] = initial
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
        print("\n模拟进度演示已停止。", flush=True)
        if receipt is not None and not finished:
            _stop_notification(args.executable, receipt.request_id)
        return 130
    except PopuError as error:
        print(f"无法运行模拟进度演示：{error}", file=sys.stderr, flush=True)
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
                "severity": "warning", "title": "模拟进度演示已停止",
                "blocks": [{"type": "paragraph", "text": "模拟进度演示被中断。未执行真实备份，未对你的备份文件做任何操作。"}],
            }, updatable=False, duration_ms=5000)
    except (PopuError, KeyboardInterrupt) as error:
        print(f"通知收尾已跳过：{error or '再次中断'}", flush=True)


if __name__ == "__main__":
    raise SystemExit(main())
