"""Command-line entry point for the installed Popu SDK demonstrations."""
from __future__ import annotations

import argparse
from typing import Sequence

from . import progress


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="python -m popu.demo",
        description="Popu SDK demonstrations. Progress and statistics are simulated; no real backup is performed.",
    )
    commands = parser.add_subparsers(dest="demo", required=True)
    commands.add_parser("progress", add_help=False, help="Simulated progress in a mock backup scenario")
    _, arguments = parser.parse_known_args(argv)
    return progress.main(arguments)


if __name__ == "__main__":
    raise SystemExit(main())
