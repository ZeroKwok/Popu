"""Send a simulated service alert that requires reading in notification history."""
import argparse
import sys

from popu import Client, PopuError


def notification() -> dict:
    return {
        "version": "1.0", "mode": "notify", "template": "notification",
        "window": {"width": 500, "height": 520},
        "duration_ms": 15000, "requires_read": True,
        "content": {
            "severity": "error", "title": "API error rate crossed 5%",
            "vertical_align": "start",
            "blocks": [
                {"type": "paragraph", "text": "SERVICE HEALTH / DEMO INCIDENT\nSearch requests are failing above the alert threshold."},
                {"type": "properties", "items": [
                    {"label": "Service", "text": "Atlas API / Production (simulated)"},
                    {"label": "Error rate", "text": "8.4% / threshold 5.0%"},
                    {"label": "Impact", "text": "84 of 1,000 requests failed"},
                    {"label": "Window", "text": "Last 5 minutes"},
                ]},
                {"type": "paragraph", "title": "Diagnostic snippet / select to copy",
                 "text": "GET /v1/search -> 503 upstream_timeout\ntrace_id=demo-7f3a | region=eu-west-demo"},
                {"type": "unordered_list", "title": "Suggested checks", "items": [
                    "Inspect upstream health and response times.",
                    "Compare failures with the latest release.",
                ]},
                {"type": "paragraph", "text": "Simulated incident. No monitoring service is contacted.\nAfter this card closes, open Popu history to review it."},
            ],
        },
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m popu.demo alert", description=__doc__)
    parser.add_argument("--wait", action="store_true", help="Wait for the notification to close, not for it to be read")
    parser.add_argument("--executable", help="Use a local Popu executable for source development")
    args = parser.parse_args(argv)
    try:
        client = Client(args.executable)
        receipt = client.notify(notification())
        print("Simulated alert sent. It requires reading in Popu history.", flush=True)
        print("Close the card, then open its detail in history to mark it read.", flush=True)
        if args.wait:
            client.wait(receipt.request_id)
            print("Notification finished. Reading it in history is a separate step.", flush=True)
    except KeyboardInterrupt:
        print("\nDemo stopped.", flush=True)
        return 130
    except PopuError as error:
        print(f"Could not run the demo: {error}", file=sys.stderr, flush=True)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
