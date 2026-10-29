# Popu Python SDK

The Python package is a client for the persistent local Popu Host. It invokes the installed `popu` CLI, parses accepted receipts and terminal snapshots, and never manages notification child processes or lifetime pipes.

## Installation

Requires Python 3.10+ and a matching `popu` executable. From the repository root:

```powershell
python -m pip install -e ./python
```

Set `Client(executable="path/to/popu.exe")` to use a local build. A command sequence such as `["path/to/popu.exe"]` is also accepted. The SDK targets the current Host protocol and does not accept pre-Host response formats.

## Requests and results

```python
from popu import Client, DialogRequest, Notification

client = Client()
result = client.show(DialogRequest.confirm(title="Continue?"))

receipt = client.notify(Notification.success(title="Done"))
snapshot = client.status(receipt.request_id)
snapshot = client.update(receipt.request_id, receipt.revision, {
    "severity": "success",
    "title": "Still done",
    "blocks": [],
})
client.dismiss(receipt.request_id)
```

`create` and `notify` return `RequestReceipt` (`NotificationReceipt` is an alias) with `status="accepted"`, a Host-generated `request_id`, `state`, `revision`, and `duplicate`. Acceptance can mean `queued`; it does not mean the window has already appeared. `create` returns after acceptance for dialogs as well as notifications; `show` waits and returns an `InteractionResult` containing `status`, `action`, and `values`.

`status`, `update`, `dismiss`, and `wait` return Host snapshot dictionaries. `wait` always reads the authoritative terminal snapshot, including `revision`, `mode`, `content`, timestamps, optional `response`, and `exit_code`. The same request can be managed from another `Client` instance or Python process using its ID. Updating requires the latest revision and replaces the entire `content`; it does not restart the display timer. Revision conflicts, missing IDs, validation failures, and subprocess/JSON errors raise `PopuError`.

Cancelled, closed, and timed-out dialogs are normal `InteractionResult` outcomes. A custom dialog action may use exit code `12` (or any other code from 0–255); the SDK reads the Host result rather than treating that business exit code as a transport error. If a terminal dialog has no response, `show` raises `PopuError` with its terminal state.

`Client.create`, `Client.show`, `Client.notify`, and the module-level `show`/`notify` helpers accept `template=Path(...)` for a trusted custom template directory. `popu` is resolved from `PATH` unless `Client(executable=...)` is supplied. CLI communication uses UTF-8 on Windows and other platforms. After accepting a dialog, the SDK drains and reaps its CLI subprocess in the background even if `wait` is never called; the Host owns the request lifetime.

## Window options

Notifications are always submitted with `window.always_on_top=True`; `Notification.success` does not expose a non-topmost option. It defaults to the primary display work area and rejects `area=ScreenArea.SCREEN`. Raw request dictionaries are validated by the CLI/Host, which reject notification `always_on_top=False`.

Dialog builders support `always_on_top`, `screen`, `area`, `width`, and `height`. Sizes are logical pixels or percentage strings, without manual DPI multiplication. Both builders support `vertical_align` and `coalesce_key`:

```python
from popu import Screen, ScreenArea, VerticalAlign

request = DialogRequest.confirm(
    title="Review", always_on_top=False,
    screen=Screen.PRIMARY, area=ScreenArea.SCREEN,
    width="80%", height="80%", vertical_align=VerticalAlign.START,
)
receipt = client.notify(Notification.success(title="Backup", coalesce_key="job:backup"))
```

For other severities, forms, or custom dialog actions, pass a complete protocol request dictionary to `create`, `show`, or `notify`. No separate Python transport or renderer is required.

## Reading requirements

Notifications accept `requires_read=True` (default: `False`):

```python
client.notify(Notification.success(title="Review backup results", requires_read=True))
```

After timeout, all notifications remain unread in history, but only those requiring reading keep the tray reminder active. Displayed or queued notifications do not trigger the tray reminder; it represents only unread history requiring reading. Viewing an individual detail in the foreground history window marks it read; opening the window does not acknowledge the whole list. Reading does not change the request's terminal state. Dialogs do not accept this field.

Both builders accept `blocks=[...]` and an optional `icon` image source. Use `image_source` to capture a local SVG/PNG/JPEG/WebP file, or `svg_source` for SVG text; the submitted data is self-contained and remains available in history.

```python
from popu import chart_block, image_block, image_source, svg_source

chart = chart_block({
    "data": {"values": [{"service": "Sync", "ms": 115}, {"service": "Backup", "ms": 78}]},
    "mark": "bar",
    "encoding": {
        "x": {"field": "service", "type": "nominal"},
        "y": {"field": "ms", "type": "quantitative"},
    },
}, title="Service latency", height=240)
client.notify(Notification.success(title="Measurements", blocks=[chart], height=400))
client.show(DialogRequest.confirm(title="Review", blocks=[chart], width=740, height=580))

# A caller-rendered graphic, including chart types outside Vega-Lite:
picture = image_block(image_source("report.svg", alt="Report graphic"), height=240)
client.notify(Notification.success(title="Report", blocks=[picture], height=400))
```

Chart blocks accept generic Vega-Lite 6 specifications with inline data and render interactive SVG without network access. Hover shows encoded field values by default; use `encoding.tooltip` to customize labels and formatting, or `null` to disable. Native Vega-Lite `params` support nearest-point hover, highlighting, legend selection, and scale-bound zoom/pan when declared in the spec. Compilation runs in a local worker; a CSP-compatible live Vega view handles rendering and interaction. Image blocks remain static. Changing the mark/encodings is sufficient for lines, bars and heatmaps. Visual `height` is within 64–1200; an image data URI is limited to 256 KiB, a chart spec to 128 KiB, and the complete content to 768 KiB. SVG scripts and external resources are rejected.
