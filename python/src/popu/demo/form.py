"""Collect a simulated release plan and print the submitted form values."""
import argparse
import json
import sys

from popu import Client, PopuError


def request() -> dict:
    return {
        "version": "1.0", "mode": "dialog", "template": "form",
        "position": "cursor_screen_center",
        "window": {"width": 500, "height": 560}, "timeout_seconds": 300,
        "content": {
            "severity": "info", "title": "Plan your next release",
            "vertical_align": "start",
            "blocks": [
                {"type": "paragraph", "text": "ATLAS API / DEMO WORKSPACE\nChoose the release settings. No deployment will run."},
                {"type": "form", "fields": [
                    {"id": "version", "type": "text", "label": "Release version",
                     "default": "2.8.0", "required": True, "pattern": r"[0-9]+\.[0-9]+\.[0-9]+", "max_length": 40},
                    {"id": "environment", "type": "select", "label": "Target environment",
                     "default": "staging", "required": True, "options": [
                         {"value": "staging", "label": "Staging / internal preview"},
                         {"value": "production", "label": "Production / simulated"},
                     ]},
                    {"id": "notes", "type": "textarea", "label": "Release notes",
                     "default": "Improve search and reduce API latency.", "max_length": 240},
                    {"id": "smoke_checks", "type": "checkbox", "label": "Run smoke checks after deployment", "default": True},
                    {"id": "reviewed", "type": "checkbox", "label": "I have reviewed this demo plan", "required": True},
                ]},
            ],
        },
        "actions": [
            {"id": "save", "label": "Create demo plan", "style": "primary", "submit": True},
            {"id": "cancel", "label": "Cancel", "style": "secondary", "result_status": "cancelled"},
        ],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m popu.demo form", description=__doc__)
    parser.add_argument("--executable", help="Use a local Popu executable for source development")
    args = parser.parse_args(argv)
    try:
        print("Waiting for a release plan. This is a simulation.", flush=True)
        result = Client(args.executable).show(request())
        if result.status == "submitted":
            print("Demo plan created. No deployment was started.", flush=True)
            print(json.dumps(dict(result.values), indent=2), flush=True)
        else:
            print(f"Plan skipped ({result.status}). No values were submitted.", flush=True)
    except KeyboardInterrupt:
        print("\nDemo stopped. No deployment was started.", flush=True)
        return 130
    except PopuError as error:
        print(f"Could not run the demo: {error}", file=sys.stderr, flush=True)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
