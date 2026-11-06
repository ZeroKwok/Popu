"""Show a simulated release pipeline and its final summary with the Popu SDK."""
import argparse
import sys
import time

from popu import Client, PopuError


def progress_content(percent: int) -> dict:
    finished = percent == 100
    tasks = 120 * percent // 100
    stages = ("Compile", "Verify", "Package")
    pipeline = [
        {"stage": stage, "completed": min(40, max(0, tasks - index * 40))}
        for index, stage in enumerate(stages)
    ]
    phase = "Ready for handoff" if finished else f"{stages[min(tasks // 40, 2)]} in progress"
    return {
        "severity": "success" if finished else "info",
        "title": "Release candidate ready" if finished else "Building the next release",
        "vertical_align": "start",
        "blocks": [
            {"type": "paragraph", "text": "RELEASE LAB / SIMULATED RUN\nAtlas Desktop / Windows x64"},
            {"type": "chart", "title": "Build stages", "engine": "vega_lite", "height": 116,
             "spec": {
                "description": "Simulated task completion across three release stages",
                "data": {"values": pipeline},
                "layer": [
                    {"mark": {"type": "bar", "color": "#94a3b8", "opacity": 0.18, "cornerRadiusEnd": 4, "size": 14},
                     "encoding": {"x": {"datum": 40, "type": "quantitative", "axis": None}}},
                    {"mark": {"type": "bar", "cornerRadiusEnd": 4, "size": 14},
                     "encoding": {"x": {"field": "completed", "type": "quantitative", "scale": {"domain": [0, 40]}, "axis": None},
                                  "color": {"field": "stage", "type": "nominal", "legend": None,
                                             "scale": {"domain": list(stages), "range": ["#4f8cff", "#a78bfa", "#2dd4bf"]}}}},
                ],
                "encoding": {
                    "y": {"field": "stage", "type": "ordinal", "sort": list(stages),
                          "axis": {"title": None, "domain": False, "ticks": False, "labelPadding": 10, "labelFontSize": 12}},
                    "tooltip": [
                        {"field": "stage", "type": "nominal", "title": "Stage"},
                        {"field": "completed", "type": "quantitative", "title": "Completed / 40 tasks"},
                    ],
                },
            }},
            {"type": "properties", "items": [
                {"label": "Completed", "text": f"{tasks} / 120 tasks"},
                {"label": "Delivered" if finished else "Deliverables", "text": "Installer, checksums, test report"},
            ]},
            {"type": "progress", "title": phase, "value": percent},
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
            "window": {"width": 480, "height": 470},
            "duration_ms": 15000,
            "updatable": True,
            "content": progress_content(0),
        })
        revision = receipt.revision

        # Each update replaces the full content and returns a new revision.
        for percent in  (8, 15, 20, 30, 40, 60, 70, 80, 95):
            time.sleep(1.5)
            snapshot = client.update(receipt.request_id, revision, progress_content(percent))
            revision = snapshot["revision"]
            print(f"Progress: {percent}%", flush=True)

        time.sleep(1.5)
        client.update(
            receipt.request_id,
            revision,
            progress_content(100),
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
                "severity": "warning", "title": "Build demo stopped",
                "blocks": [{"type": "paragraph", "text": "Simulated build interrupted. No build commands were executed."}],
            }, updatable=False, duration_ms=5000)
    except (PopuError, KeyboardInterrupt) as error:
        print(f"Notification cleanup skipped: {error or 'interrupted again'}", flush=True)


if __name__ == "__main__":
    raise SystemExit(main())
