# Popu

Desktop notifications and lightweight dialogs for Python scripts and automation tools.

Show task progress, report results, ask for confirmation, and keep notification history available after your script exits. The package includes the Python SDK and the `popu` command-line tool. Everything runs locally; no external service or account is needed.

## Install and try it

Currently supported: **Windows x64 with 64-bit Python 3.10+**. [Microsoft WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) is required; if it is missing, Popu provides a manual installation link.

```sh
python -m pip install popu
python -m popu.demo progress
```

The demo shows a progress notification followed by a result summary. It simulates a backup without accessing your files. Try closing the progress card: updates continue in the background, and the final summary requests one more presentation. Press Ctrl+C to stop the demo. Add `--wait` to wait for the final notification to finish displaying. Console messages are in English.

The native executable is included. You can run the installed package from any directory without checking out the repository or installing build tools. The background Host starts automatically on the first request and keeps notifications and history available after the submitting script exits.

## Send a notification

```python
from popu import Client, Notification

client = Client()
client.notify(
    Notification.success(
        title="Backup completed",
        message="128 files backed up successfully.",
    )
)
```

## Ask for confirmation

```python
from popu import Client, DialogRequest

result = Client().show(
    DialogRequest.confirm(
        title="Deploy release?",
        message="The release is ready for production.",
    )
)

if result.status == "submitted" and result.action == "confirm":
    print("Deployment confirmed")
```

`show()` waits for the user's response. Cancelling, closing, or timing out are normal outcomes; check the result before continuing your task.

## Update task progress

This example simulates progress; replace the sleeps and percentages with your task's own reports.

```python
import time
from popu import Client

client = Client()
receipt = client.notify({
    "version": "1.0",
    "mode": "notify",
    "template": "notification",
    "updatable": True,
    "content": {
        "severity": "info",
        "title": "Example task",
        "blocks": [{"type": "progress", "value": 0}],
    },
})
revision = receipt.revision

for percent in (25, 50, 75, 100):
    time.sleep(1)
    done = percent == 100
    snapshot = client.update(
        receipt.request_id,
        revision,
        {
            "severity": "success" if done else "info",
            "title": "Example completed" if done else "Example task",
            "blocks": [{"type": "progress", "value": percent}],
        },
        updatable=not done,
        reshow=done,
    )
    revision = snapshot["revision"]
```

- Set `updatable=True` when creating a notification you intend to update.
- Each update replaces the full content and uses the latest revision.
- Finish with `updatable=False` to save the final content and stop accepting updates.
- Updates restart the display timer. Closing a card silences ordinary updates; `reshow=True` explicitly requests another presentation, allowed once beyond the initial presentation by default. After an automatic timeout, ordinary updates can show it again.

See the [notification lifecycle](https://github.com/ZeroKwok/Popu/blob/master/docs/live-notifications.md) for complete rules.

## Use the command line

```sh
popu --help
popu templates list
```

Save this request as `notification.json`:

```json
{
  "version": "1.0",
  "mode": "notify",
  "template": "notification",
  "content": {
    "severity": "success",
    "title": "Task completed",
    "blocks": [{"type": "paragraph", "text": "Your results are ready."}]
  }
}
```

```sh
popu request notification.json
```

The CLI and SDK use the same local Host and notification history. See the [JSON protocol](https://github.com/ZeroKwok/Popu/blob/master/protocol/popu.schema.json) for other request types.

## Troubleshooting

- **`popu` is not found:** use `python -m popu` with the same arguments, or add your Python scripts directory to `PATH`.
- **WebView2 is missing:** install the runtime from [Microsoft](https://developer.microsoft.com/microsoft-edge/webview2/) and try again. Popu does not install it automatically.
- **An older Host is still running after an upgrade:** inspect it with `python -m popu host status`. When you are ready to end its active interactions, exit it from the tray or run `python -m popu host stop`, then retry. The next request starts the new Host.

To upgrade the package, run `python -m pip install --upgrade popu`.

## Documentation

- [SDK reference](https://github.com/ZeroKwok/Popu/blob/master/docs/python-sdk.md): request results, window options, images, charts, and runtime behavior.
- [Demo guide](https://github.com/ZeroKwok/Popu/blob/master/python/src/popu/demo/README.md): available examples, options, and source development.
- [Project overview](https://github.com/ZeroKwok/Popu#readme) and [issue tracker](https://github.com/ZeroKwok/Popu/issues).
- [Build and publishing](https://github.com/ZeroKwok/Popu/blob/master/docs/python-distribution.md): for contributors building from source.

## License

[Apache License 2.0](https://github.com/ZeroKwok/Popu/blob/master/LICENSE).
