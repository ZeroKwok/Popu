"""Show a simulated workspace backup and its final summary with the Popu SDK."""
import argparse
import sys
import time

from popu import Client, PopuError


def progress_content(percent: int) -> dict:
    return {
        "severity": "info",
        "title": "Backing up your workspace",
        "blocks": [
            {"type": "paragraph", "text": "Documents, photos, and project files.\nSimulation only."},
            {"type": "properties", "items": [
                {"label": "Files", "text": f"{120 * percent // 100} / 120"},
                {"label": "Data", "text": f"{2.4 * percent / 100:.1f} / 2.4 GB"},
                {"label": "Destination", "text": "Local vault / Daily snapshot"},
            ]},
            {"type": "progress", "title": "Creating daily snapshot", "value": percent},
        ],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m popu.demo progress", description=__doc__)
    parser.add_argument("--wait", action="store_true", help="Wait for the final notification to close")
    parser.add_argument("--executable", help="Use a local Popu executable for source development")
    args = parser.parse_args(argv)

    receipt = None
    try:
        print("Simulated progress demo. No files will be modified.", flush=True)
        client = Client(args.executable)
        receipt = client.notify({
            "version": "1.0",
            "mode": "notify",
            "template": "notification",
            "window": {"width": 440, "height": 350},
            "duration_ms": 15000,
            "updatable": True,
            "content": progress_content(0),
        })
        revision = receipt.revision

        # Each update replaces the full content and returns a new revision.
        for percent in (20, 40, 60, 80):
            time.sleep(1)
            snapshot = client.update(receipt.request_id, revision, progress_content(percent))
            revision = snapshot["revision"]
            print(f"Progress: {percent}%", flush=True)

        time.sleep(1)
        final_content = {
            "severity": "success",
            "title": "Workspace backup complete",
            "blocks": [
                {"type": "paragraph", "text": "Daily snapshot completed.\nSimulation only. "},
                {"type": "properties", "items": [
                    {"label": "Files", "text": "120 backed up"},
                    {"label": "Data", "text": "2.4 GB"},
                    {"label": "Destination", "text": "Local vault / Daily snapshot"},
                ]},
                {"type": "progress", "title": "Snapshot ready", "value": 100},
            ],
        }
        client.update(
            receipt.request_id,
            revision,
            final_content,
            updatable=False,
            reshow=True,
            duration_ms=10000,
        )
        print("Progress: 100%\nDemo completed.", flush=True)
        if args.wait:
            client.wait(receipt.request_id)
            print("Final notification finished.", flush=True)
    except KeyboardInterrupt:
        print("\nDemo stopped.", flush=True)
        if receipt is not None:
            _stop_notification(args.executable, receipt.request_id)
        return 130
    except PopuError as error:
        print(f"Could not run the demo: {error}", file=sys.stderr, flush=True)
        return 1
    return 0


def _stop_notification(executable: str | None, request_id: str) -> None:
    # An interrupted update may already be accepted. Read its latest revision;
    # use bounded cleanup so stopping the example cannot hang.
    try:
        client = Client(executable, timeout=1)
        snapshot = client.status(request_id)
        if snapshot.get("updatable"):
            client.update(request_id, snapshot["revision"], {
                "severity": "warning", "title": "Backup demo stopped",
                "blocks": [{"type": "paragraph", "text": "Simulation interrupted. No files were modified."}],
            }, updatable=False, duration_ms=5000)
    except (PopuError, KeyboardInterrupt) as error:
        print(f"Notification cleanup skipped: {error or 'interrupted again'}", flush=True)


if __name__ == "__main__":
    raise SystemExit(main())
