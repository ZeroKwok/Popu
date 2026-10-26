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
