"""Review a simulated release and handle approval, cancellation, or timeout."""
import argparse
import sys

from popu import Client, DialogRequest, PopuError, VerticalAlign


def request() -> DialogRequest:
    return DialogRequest.confirm(
        title="Ready to ship Atlas API?",
        message="RELEASE REVIEW / DEMO\nA small update is ready for your approval.",
        blocks=[
            {"type": "properties", "items": [
                {"label": "Environment", "text": "Production (simulated)"},
                {"label": "Version", "text": "2.7.4 -> 2.8.0"},
                {"label": "Rollout", "text": "3 instances / rolling update"},
                {"label": "Checks", "text": "128 tests passed / rollback ready"},
            ]},
            {"type": "unordered_list", "title": "What changes", "items": [
                "Faster search and lower API latency.",
                "No database migration or planned downtime.",
            ]},
            {"type": "paragraph", "text": "Demo only. Approving prints a message; nothing is deployed."},
        ],
        confirm_label="Approve release", cancel_label="Keep current version",
        timeout_seconds=180, width=500, height=430,
        vertical_align=VerticalAlign.START,
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m popu.demo confirm", description=__doc__)
    parser.add_argument("--executable", help="Use a local Popu executable for source development")
    args = parser.parse_args(argv)
    try:
        print("Waiting for release approval. This is a simulation.", flush=True)
        result = Client(args.executable).show(request())
        if result.status == "submitted":
            print("Release approved. Demo only; nothing was deployed.", flush=True)
        else:
            print(f"Release skipped ({result.status}). Nothing was deployed.", flush=True)
    except KeyboardInterrupt:
        print("\nDemo stopped. Nothing was deployed.", flush=True)
        return 130
    except PopuError as error:
        print(f"Could not run the demo: {error}", file=sys.stderr, flush=True)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
