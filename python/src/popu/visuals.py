"""Self-contained visual blocks shared by dialogs and notifications."""
from __future__ import annotations

import base64
from pathlib import Path
from typing import Any, Mapping


def _source(data: bytes, media_type: str, alt: str) -> dict[str, str]:
    if not alt.strip() or len(alt) > 1000:
        raise ValueError("alt must contain 1-1000 characters")
    src = f"data:{media_type};base64,{base64.b64encode(data).decode('ascii')}"
    if not data or len(src) > 256 * 1024:
        raise ValueError("image must be nonempty and its data URI must not exceed 256 KiB")
    return {"src": src, "alt": alt}


def image_source(path: str | Path, *, alt: str) -> dict[str, str]:
    """Read an image now so moving/deleting the file cannot break history."""
    path = Path(path)
    media_types = {".svg": "image/svg+xml", ".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".webp": "image/webp"}
    if path.suffix.lower() not in media_types:
        raise ValueError("supported image types are SVG, PNG, JPEG and WebP")
    with path.open("rb") as stream:
        data = stream.read(192 * 1024 + 1)
    return _source(data, media_types[path.suffix.lower()], alt)


def svg_source(svg: str, *, alt: str) -> dict[str, str]:
    return _source(svg.encode("utf-8"), "image/svg+xml", alt)


def image_block(source: Mapping[str, str], *, title: str | None = None,
                caption: str | None = None, height: int | None = None) -> dict[str, Any]:
    block: dict[str, Any] = {"type": "image", "source": dict(source)}
    for key, value in {"title": title, "caption": caption, "height": height}.items():
        if value is not None:
            block[key] = value
    return block


def chart_block(spec: Mapping[str, Any], *, title: str | None = None,
                height: int | None = None) -> dict[str, Any]:
    block: dict[str, Any] = {"type": "chart", "engine": "vega_lite", "spec": dict(spec)}
    if title is not None:
        block["title"] = title
    if height is not None:
        block["height"] = height
    return block
