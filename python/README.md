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
