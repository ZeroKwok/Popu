#!/usr/bin/env python3
"""Generate Popu application and tray icons from the SVG source of truth."""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
ICON_DIR = REPO_ROOT / "src-tauri" / "icons"
TRAY_DIR = ICON_DIR / "tray"
SOURCE_SVG = ICON_DIR / "icon.svg"

COMMITTED_PNGS = (
    (16, "16x16.png"),
    (20, "20x20.png"),
    (32, "32x32.png"),
    (48, "48x48.png"),
    (64, "64x64.png"),
    (128, "128x128.png"),
    (256, "128x128@2x.png"),
)
ICO_SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)
TRAY_FRAMES = (
    ("idle",            205, 196, 15, 15, "#78E3B5"),  # 静止
    ("takeoff",         205, 204, 20, 11, "#78E3B5"),  # 起飞
    ("rise",            201, 174, 18, 21, "#45D6C4"),  # 上升
    ("peak",            194, 147, 21, 21, "#FFBE4F"),  # 顶点
    ("fall",            200, 175, 23, 23, "#FF8A4C"),  # 下落
    ("land",            205, 204, 28, 16, "#FF5C62"),  # 着陆
    ("active",          205, 190, 24, 24, "#FF5C62"),  # 活跃
    ("pulse-crouch",    205, 202, 28, 16, "#FF5C62"),  # 脉冲下蹲
    ("pulse-peak",      197, 158, 23, 25, "#FF5C62"),  # 脉冲顶点
    ("pulse-land",      205, 204, 28, 16, "#FF5C62"),  # 脉冲着陆
)
STATUS_DOT = re.compile(r'<ellipse id="status-dot"[^>]*/>')


def run_magick(executable: str, *arguments: object) -> None:
    subprocess.run([executable, *(str(argument) for argument in arguments)], check=True)


def export_png(executable: str, source: Path, size: int, destination: Path) -> None:
    run_magick(
        executable,
        "-background",
        "none",
        source,
        "-resize",
        f"{size}x{size}",
        "-strip",
        f"PNG32:{destination}",
    )


def generate(executable: str) -> None:
    if shutil.which(executable) is None:
        raise SystemExit(f"ImageMagick command {executable!r} was not found")
    if not SOURCE_SVG.is_file():
        raise SystemExit(f"Missing SVG source: {SOURCE_SVG}")

    TRAY_DIR.mkdir(parents=True, exist_ok=True)
    source = SOURCE_SVG.read_text(encoding="utf-8")

    # with tempfile.TemporaryDirectory(prefix="popu-icons-") as temporary:
    #     temporary = Path(temporary)
    temporary = REPO_ROOT / ".assets"
    temporary.mkdir(parents=True, exist_ok=True)
    if temporary.exists():

        for size, name in COMMITTED_PNGS:
            export_png(executable, SOURCE_SVG, size, ICON_DIR / name)

        ico_inputs: list[Path] = []
        for size in ICO_SIZES:
            path = temporary / f"ico-{size}.png"
            export_png(executable, SOURCE_SVG, size, path)
            ico_inputs.append(path)
        run_magick(executable, *ico_inputs, ICON_DIR / "icon.ico")

        for name, cx, cy, rx, ry, color in TRAY_FRAMES:
            ellipse = (
                f'<ellipse id="status-dot" cx="{cx}" cy="{cy}" '
                f'rx="{rx}" ry="{ry}" fill="{color}"/>'
            )
            frame_svg, replacements = STATUS_DOT.subn(ellipse, source)
            if replacements != 1:
                raise RuntimeError(f"Expected one status-dot ellipse, found {replacements}")
            temporary_svg = temporary / f"tray-{name}.svg"
            temporary_svg.write_text(frame_svg, encoding="utf-8", newline="\n")
            export_png(executable, temporary_svg, 32, TRAY_DIR / f"{name}.png")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--magick", default="magick", help="ImageMagick executable (default: magick)")
    arguments = parser.parse_args()
    generate(arguments.magick)


if __name__ == "__main__":
    main()
