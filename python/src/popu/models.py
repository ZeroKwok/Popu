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


WindowDimension = int | float | str


def _window_options(
    *,
    screen: Screen,
    area: ScreenArea,
    width: WindowDimension | None,
    height: WindowDimension | None,
) -> dict[str, Any]:
    options: dict[str, Any] = {"screen": screen.value, "area": area.value}
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
        position: Position = Position.CURSOR_SCREEN_CENTER,
        confirm_label: str = "Confirm",
        cancel_label: str = "Cancel",
        dangerous: bool = False,
        timeout_seconds: int = 300,
        screen: Screen = Screen.CURSOR,
        area: ScreenArea = ScreenArea.WORK_AREA,
        width: WindowDimension | None = None,
        height: WindowDimension | None = None,
    ) -> "DialogRequest":
        return cls(
            {
                "version": "1.0",
                "mode": "dialog",
                "template": "confirm",
                "position": position.value,
                "window": _window_options(
                    screen=screen, area=area, width=width, height=height
                ),
                "content": {"severity": "warning" if dangerous else "info", "title": title, "message": message},
                "form": {"fields": []},
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
        position: Position = Position.BOTTOM_RIGHT,
        duration_ms: int = 8_000,
        screen: Screen = Screen.CURSOR,
        area: ScreenArea = ScreenArea.WORK_AREA,
        width: WindowDimension | None = None,
        height: WindowDimension | None = None,
    ) -> "Notification":
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
                "content": {"severity": "success", "title": title, "message": message},
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
