# Popu Python SDK

The Python package is a client for the persistent local Popu Host. It invokes the installed `popu` CLI, parses accepted receipts and terminal snapshots, and never manages notification child processes or lifetime pipes.

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

`Client.create`, `update`, `dismiss`, `status`, and `wait` expose Host-generated `request_id`, revision, queue state, and duplicate information. `popu` is resolved from `PATH` unless `Client(executable=...)` is supplied.

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
