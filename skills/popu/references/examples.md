# Popu integration examples

Use these patterns for SDK calls, raw CLI requests, live updates, and visuals. Replace illustrative titles, counts, and consequences with the caller's actual data. The installed SDK needs no repository checkout.

## Install and inspect

When setup is in scope:

```sh
python -m pip install popu
python -m popu --version
python -m popu --help
python -m popu templates list
```

Currently supported: Windows x64, 64-bit Python 3.10+, and [Microsoft WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/). Popu supplies a manual installation link when the runtime is missing; it does not install WebView2 automatically.

The packaged simulation is `python -m popu.demo progress`; add `--wait` to wait for the final presentation. It does not operate on backup files.

## Python notification

```python
from popu import Client, Notification

receipt = Client().notify(
    Notification.success(title="Export completed", message="The requested report is ready.")
)
print(receipt.request_id)
```

The receipt reports acceptance, not a user's acknowledgement or proof of presentation. Add `requires_read=True` only if this result should keep a tray reminder while unread in history.

## Python decision

```python
from popu import Client, DialogRequest

result = Client().show(
    DialogRequest.confirm(title="Publish the report?", message="The report will become visible to the team.")
)
if result.status == "submitted" and result.action == "confirm":
    print("Publication confirmed")
else:
    print("No publication requested")
```

Execute the corresponding business operation only after interpreting the actual result and existing task authorization. For forms or custom actions, pass a complete request mapping to `create()` or `show()`. `create()` returns a receipt; use `wait()` to obtain a terminal snapshot containing `response`, not an `InteractionResult` directly.

## Python live progress

This runnable example simulates a task. In a real integration, replace the sleeps and percentages with the task's measurements and send its actual final outcome.

```python
import time
from popu import Client

client = Client()
receipt = client.notify({
    "version": "1.0",
    "mode": "notify",
    "template": "notification",
    "updatable": True,
    "duration_ms": 20000,
    "content": {
        "severity": "info",
        "title": "Simulated task",
        "blocks": [{"type": "progress", "value": 0}],
    },
})
revision = receipt.revision
for percent in (25, 50, 75, 100):
    time.sleep(1)
    finished = percent == 100
    snapshot = client.update(
        receipt.request_id,
        revision,
        {
            "severity": "success" if finished else "info",
            "title": "Simulation completed" if finished else "Simulated task",
            "blocks": [{"type": "progress", "value": percent}],
        },
        updatable=not finished,
        reshow=finished,
        **({"duration_ms": 10000} if finished else {}),
    )
    revision = snapshot["revision"]
```

This example chooses an explicit final reshow. An integration may instead seal silently. Do not resubmit another notification if a final reshow is suppressed. Call `client.status(receipt.request_id)` to reconcile a revision conflict; do not assume the interrupted update failed to reach the Host. Call `dismiss()` only to cancel the notification itself.

## Python visuals

```python
from popu import Client, Notification, chart_block, image_block, svg_source

chart = chart_block({
    "data": {"values": [{"stage": "Build", "seconds": 18}, {"stage": "Test", "seconds": 12}]},
    "mark": "bar",
    "encoding": {
        "x": {"field": "stage", "type": "nominal"},
        "y": {"field": "seconds", "type": "quantitative"},
    },
}, title="Stage durations", height=220)
image = image_block(
    svg_source('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 24"><rect width="120" height="24" fill="#247a48"/></svg>', alt="Green completion indicator"),
    height=64,
)
Client().notify(Notification.success(title="Example measurements", blocks=[chart, image], height=460))
```

For a caller-rendered report image, use `image_source("report.svg", alt="Report summary")` instead of `svg_source()`. Charts support inline Vega-Lite composition and declared interactions; use `params` or `encoding.tooltip` when the summary needs them. All data and image content must fit the limits in the skill.

## Raw CLI completion notification

Save as a caller-controlled request file and invoke `popu request <file>`:

```json
{
  "version": "1.0",
  "mode": "notify",
  "template": "notification",
  "position": "bottom_right",
  "duration_ms": 6000,
  "content": {
    "severity": "success",
    "title": "Backup completed",
    "blocks": [
      { "type": "paragraph", "text": "128 files copied in 3 minutes 21 seconds." },
      { "type": "properties", "items": [{ "label": "Size", "text": "2.4 GB" }] },
      { "type": "progress", "title": "Completion", "value": 100 }
    ]
  }
}
```

## Raw CLI destructive confirmation

```json
{
  "version": "1.0",
  "mode": "dialog",
  "template": "confirm",
  "position": "cursor_screen_center",
  "content": {
    "severity": "warning",
    "title": "Replace 3 existing files?",
    "blocks": [
      { "type": "paragraph", "text": "The destination contains files with the same names." },
      { "type": "properties", "items": [{ "label": "Destination", "text": "D:\\Backup" }] },
      {
        "type": "form",
        "fields": [{
          "id": "confirmed",
          "type": "checkbox",
          "label": "I understand existing files will be replaced",
          "required": true
        }]
      }
    ]
  },
  "actions": [
    { "id": "replace", "label": "Replace", "style": "danger", "submit": true },
    { "id": "cancel", "label": "Cancel", "style": "secondary", "result_status": "cancelled" }
  ],
  "timeout_seconds": 300
}
```

For both requests, the first stdout line is the acceptance receipt. Only the dialog produces a later terminal response; require the intended affirmative status/action and validated form values before replacing files. Cancellation, close, timeout, or a missing response does not authorize the operation.

## CLI management and updates

```text
popu status <request-id>
popu update <request-id> <update.json>
popu wait <request-id>
popu dismiss <request-id>
```

An update file contains `expected_revision` and a complete `content`; notification updates may also include `updatable`, `reshow`, and `duration_ms`. The request ID is the CLI argument, not a field in that file. For dialogs, do not add notification-only update options. Use [the update schema](https://github.com/ZeroKwok/Popu/blob/master/protocol/update.schema.json) for the exact contract.

The [English SDK reference](https://github.com/ZeroKwok/Popu/blob/master/docs/python-sdk.en.md) covers additional builders and result types. In a source checkout, [protocol examples](https://github.com/ZeroKwok/Popu/tree/master/protocol/examples) provide complete form and visual requests.
