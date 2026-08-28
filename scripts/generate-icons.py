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
SMALL_ICON_SIZES = frozenset((16, 20))
TRAY_SIZE = 16
TRAY_FRAMES = (
    # name, center x, top edge, radius x/y, color
    ("0-idle",          205, 181, 15, 15, "#78E3B5"),  # 静止
    ("1-takeoff",       205, 193, 20, 11, "#78E3B5"),  # 起飞
    ("2-rise",          201, 105, 18, 21, "#45D6C4"),  # 上升
    ("3-peak",          194,  51, 21, 21, "#FFBE4F"),  # 顶点
    ("4-fall",          200, 107, 23, 23, "#FF8A4C"),  # 下落
    ("5-land",          205, 188, 28, 16, "#FF5C62"),  # 着陆
    ("6-active",        205, 166, 24, 24, "#FF5C62"),  # 活跃
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


def replace_status_dot(source: str, cx: int, cy: int, rx: int, ry: int, color: str) -> str:
    ellipse = (
        f'<ellipse id="status-dot" cx="{cx}" cy="{cy}" '
        f'rx="{rx}" ry="{ry}" fill="{color}"/>'
    )
    result, replacements = STATUS_DOT.subn(ellipse, source)
    if replacements != 1:
        raise RuntimeError(f"Expected one status-dot ellipse, found {replacements}")
    return result


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
        small_icon_svg = temporary / "icon-small-status.svg"
        small_icon_svg.write_text(
            replace_status_dot(source, 205, 196, 34, 34, "#78E3B5"),
            encoding="utf-8",
            newline="\n",
        )

        for size, name in COMMITTED_PNGS:
            icon_source = small_icon_svg if size in SMALL_ICON_SIZES else SOURCE_SVG
            export_png(executable, icon_source, size, ICON_DIR / name)

        ico_inputs: list[Path] = []
        for size in ICO_SIZES:
            path = temporary / f"ico-{size}.png"
            icon_source = small_icon_svg if size in SMALL_ICON_SIZES else SOURCE_SVG
            export_png(executable, icon_source, size, path)
            ico_inputs.append(path)
        run_magick(executable, *ico_inputs, ICON_DIR / "icon.ico")

        for obsolete_size in ("16", "20"):
            shutil.rmtree(TRAY_DIR / obsolete_size, ignore_errors=True)
        expected_tray_files = {f"{name}.png" for name, *_ in TRAY_FRAMES}
        for existing in TRAY_DIR.glob("*.png"):
            if existing.name not in expected_tray_files:
                existing.unlink()
        scale = 2
        for name, cx, top, rx, ry, color in TRAY_FRAMES:
            scaled_rx, scaled_ry = rx * scale, ry * scale
            scaled_cx = min(cx, 244 - scaled_rx)
            frame_svg = replace_status_dot(source, scaled_cx, top + scaled_ry, scaled_rx, scaled_ry, color)
            temporary_svg = temporary / f"tray-{name}.svg"
            temporary_svg.write_text(frame_svg, encoding="utf-8", newline="\n")
            export_png(executable, temporary_svg, TRAY_SIZE, TRAY_DIR / f"{name}.png")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--magick", default="magick", help="ImageMagick executable (default: magick)")
    arguments = parser.parse_args()
    generate(arguments.magick)


if __name__ == "__main__":
    main()
