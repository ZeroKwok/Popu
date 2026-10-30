from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Any, Mapping


class Position(str, Enum):
    BOTTOM_RIGHT = "bottom_right"
    CURSOR_SCREEN_CENTER = "cursor_screen_center"


class Screen(str, Enum):
    CURSOR = "cursor"
    PRIMARY = "primary"


class ScreenArea(str, Enum):
    WORK_AREA = "work_area"
    SCREEN = "screen"


class VerticalAlign(str, Enum):
    START = "start"
    CENTER = "center"


WindowDimension = int | float | str


def _content(severity: str, title: str, message: str,
             blocks: list[Mapping[str, Any]] | None, icon: Mapping[str, str] | None,
             vertical_align: VerticalAlign) -> dict[str, Any]:
    content: dict[str, Any] = {
        "severity": severity, "title": title,
        "vertical_align": vertical_align.value,
        "blocks": ([{"type": "paragraph", "text": message}] if message else []) + [dict(block) for block in blocks or []],
    }
    if icon is not None:
        content["icon"] = dict(icon)
    return content


def _window_options(
    *,
    screen: Screen,
    area: ScreenArea,
    width: WindowDimension | None,
    height: WindowDimension | None,
    always_on_top: bool = True,
) -> dict[str, Any]:
    options: dict[str, Any] = {
        "always_on_top": always_on_top, "screen": screen.value, "area": area.value,
    }
    if width is not None:
        options["width"] = width
    if height is not None:
        options["height"] = height
    return options


@dataclass(frozen=True)
class DialogRequest:
    data: Mapping[str, Any]

    @classmethod
    def confirm(
        cls,
        *,
        title: str,
        message: str = "",
        blocks: list[Mapping[str, Any]] | None = None,
        icon: Mapping[str, str] | None = None,
        position: Position = Position.CURSOR_SCREEN_CENTER,
        confirm_label: str = "Confirm",
        cancel_label: str = "Cancel",
        dangerous: bool = False,
        timeout_seconds: int = 300,
        screen: Screen = Screen.CURSOR,
        area: ScreenArea = ScreenArea.WORK_AREA,
        width: WindowDimension | None = None,
        height: WindowDimension | None = None,
        always_on_top: bool = True,
        vertical_align: VerticalAlign = VerticalAlign.CENTER,
        coalesce_key: str | None = None,
    ) -> "DialogRequest":
        return cls(
            {
                "version": "1.0",
                "mode": "dialog",
                "template": "confirm",
                "position": position.value,
                "window": _window_options(
                    screen=screen, area=area, width=width, height=height,
                    always_on_top=always_on_top,
                ),
                "content": _content("warning" if dangerous else "info", title, message, blocks, icon, vertical_align),
                **({"coalesce_key": coalesce_key} if coalesce_key is not None else {}),
                "actions": [
                    {
                        "id": "confirm",
                        "label": confirm_label,
                        "style": "danger" if dangerous else "primary",
                        "submit": True,
                    },
                    {"id": "cancel", "label": cancel_label, "style": "secondary", "result_status": "cancelled"},
                ],
                "timeout_seconds": timeout_seconds,
            }
        )

    def as_dict(self) -> dict[str, Any]:
        return dict(self.data)


@dataclass(frozen=True)
class Notification:
    data: Mapping[str, Any]

    @classmethod
    def success(
        cls,
        *,
        title: str,
        message: str = "",
        blocks: list[Mapping[str, Any]] | None = None,
        icon: Mapping[str, str] | None = None,
        position: Position = Position.BOTTOM_RIGHT,
        duration_ms: int = 8_000,
        requires_read: bool = False,
        updatable: bool = False,
        screen: Screen = Screen.PRIMARY,
        area: ScreenArea = ScreenArea.WORK_AREA,
        width: WindowDimension | None = None,
        height: WindowDimension | None = None,
        vertical_align: VerticalAlign = VerticalAlign.CENTER,
        coalesce_key: str | None = None,
    ) -> "Notification":
        if area != ScreenArea.WORK_AREA:
            raise ValueError("notify windows use the primary display work_area")
        return cls(
            {
                "version": "1.0",
                "mode": "notify",
                "template": "notification",
                "position": position.value,
                "window": _window_options(
                    screen=screen, area=area, width=width, height=height
                ),
                "duration_ms": duration_ms,
                "requires_read": requires_read,
                "updatable": updatable,
                "content": _content("success", title, message, blocks, icon, vertical_align),
                **({"coalesce_key": coalesce_key} if coalesce_key is not None else {}),
            }
        )

    def as_dict(self) -> dict[str, Any]:
        return dict(self.data)


@dataclass(frozen=True)
class InteractionResult:
    status: str
    action: str | None
    values: Mapping[str, Any]

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "InteractionResult":
        return cls(str(value["status"]), value.get("action"), dict(value.get("values", {})))
