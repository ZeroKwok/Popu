"""Command-line entry point for the installed Popu SDK demonstrations."""
from __future__ import annotations

import argparse
from typing import Sequence

from . import alert, confirm, form, gold, progress


DEMOS = {"progress": progress, "gold": gold, "confirm": confirm, "form": form, "alert": alert}


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="python -m popu.demo",
        description="Popu SDK demonstrations. Choose an example to run.",
    )
    commands = parser.add_subparsers(dest="demo", required=True)
    for name, demo in DEMOS.items():
        commands.add_parser(name, add_help=False, help=demo.__doc__.splitlines()[0])
    selected, arguments = parser.parse_known_args(argv)
    return DEMOS[selected.demo].main(arguments)


if __name__ == "__main__":
    raise SystemExit(main())
