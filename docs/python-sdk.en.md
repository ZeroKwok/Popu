# Python SDK reference

[简体中文](python-sdk.md)

For installation, runnable examples, and troubleshooting, start with the [Python package guide](../python/README.en.md). This reference covers request handling, notification lifetimes, window options, visuals, and runtime behavior.

## Requests and results

```python
from popu import Client, DialogRequest, Notification

client = Client()
result = client.show(DialogRequest.confirm(title="Continue?"))

receipt = client.notify(Notification.success(title="Backup running", updatable=True))
snapshot = client.status(receipt.request_id)
snapshot = client.update(receipt.request_id, receipt.revision, {
    "severity": "success",
    "title": "Backup 50%",
    "blocks": [],
})
client.dismiss(receipt.request_id)
```

`create` and `notify` return `RequestReceipt` (`NotificationReceipt` is an alias) with `status="accepted"`, a Host-generated `request_id`, `state`, `revision`, and `duplicate`. Acceptance can mean `queued`; it does not mean the window has already appeared. `create` returns after acceptance for dialogs as well as notifications; `show` waits and returns an `InteractionResult` containing `status`, `action`, and `values`.

`status`, `update`, `dismiss`, and `wait` return Host snapshot dictionaries. `wait` reads the authoritative terminal snapshot, including revision, content, timestamps, response and exit code; a hidden updatable notification is not terminal. The same request can be managed from another client using its ID. Updating requires the latest revision and replaces the entire content. Every accepted notification update restarts its display timer using the configured duration; optional `duration_ms` overrides that duration. Updates preserve a dialog's original interaction deadline. Revision conflicts, missing IDs, validation failures, sealed updates and subprocess/JSON errors raise `PopuError`.

`revision` identifies the content accepted by the Host; `expected_revision` detects update conflicts. The snapshot's `acknowledged_revision` is maintained internally by the Host for close or caller-dismissal acknowledgements and cannot be supplied in requests or updates. User close acknowledges only the version already presented at the time of the action, leaving unseen updates unread. History viewing acknowledges revisions separately.

Dialog actions are disabled while replacement content prepares and must match the latest presented revision. A conflict preserves entered values and asks the user to review again, without confirming the new version. Enter in a text or number input submits only when exactly one submit action exists, validating all form blocks. Multiple submit actions require a button choice; textarea Enter retains a newline. Esc cancels dialogs; notifications do not support Esc dismissal.

`status` / `wait` query the current Host Registry, not archived history. Restart restores remaining active notification checkpoints, not archived IDs or dialogs. Retained history does not guarantee that an old ID remains queryable; clearing hidden history also removes its ID. The SQLite limit of 500 archived rows does not bound terminal records in Host memory.

Currently, persistence can fail after the Registry or window has changed, and the Host retries saving in the background. On `persistence_failed`, reconcile a known ID's revision and content before retrying. When creation returned no ID, a retry can create a duplicate. The original `coalesce_key` deduplicates only still-active requests, not terminal ones.

## Live notifications

Notifications default to `updatable=False` and reject updates. Opt in at creation for progress reports. Update omission preserves this flag; the last report sets `updatable=False`, atomically saving the final contents and rejecting further updates. There is no `final` field. Dialog content updates do not use notification options.

```python
receipt = client.notify(Notification.success(title="Backing up", updatable=True))
snapshot = client.update(receipt.request_id, receipt.revision, {
    "title": "Backup 50%", "blocks": [{"type": "progress", "value": 50}],
})
snapshot = client.update(receipt.request_id, snapshot["revision"], {
    "severity": "success", "title": "Backup completed",
    "blocks": [{"type": "paragraph", "text": "2,000 files verified."}],
}, updatable=False, reshow=True, duration_ms=10000)
```

User close hides progress and silences ordinary updates. An explicit `reshow=True` permits one further presentation by default; `reshow_suppressed` reports a consumed budget without rejecting the content. Automatic display timeout hides the card, and the next report shows it again unless the user previously closed it. `dismiss` is caller cancellation and ends the notification outright.

The default template displays occurrence time, refreshed on every accepted report. Hidden contents remain current in history. After 12 hours without a successful report, the notification becomes `stale` and rejects updates. Host restart restores live records hidden without resetting this deadline. See the [complete lifecycle (Chinese)](https://github.com/ZeroKwok/Popu/blob/HEAD/docs/live-notifications.md) and [installed progress demo (Chinese)](https://github.com/ZeroKwok/Popu/blob/HEAD/python/src/popu/demo/README.md).

Cancelled, closed, and timed-out dialogs are normal `InteractionResult` outcomes. A custom dialog action may use exit code `12` (or any other code from 0–255); the SDK reads the Host result rather than treating that business exit code as a transport error. If a terminal dialog has no response, `show` raises `PopuError` with its terminal state.

`Client.create`, `Client.show`, `Client.notify`, and the module-level `show`/`notify` helpers accept `template=Path(...)` for a trusted custom template directory. The default client uses its bundled version rather than searching `PATH`; `Client(executable=...)` overrides this explicitly. CLI communication uses UTF-8 JSON lines through a stdout pipe; there is no stdout temporary file or receipt polling. Notifications and management commands read their response line before waiting for the CLI to exit, without waiting for trailing pipe EOF. Stderr uses a temporary file so diagnostic bursts cannot block receipt delivery. After accepting a dialog, the SDK drains remaining stdout in bounded chunks and reaps its CLI in the background even if `wait` is never called; the Host owns the request lifetime and is launched without inherited CLI handles.

CLI response reads and process waits remain interruptible on Windows: the blocking pipe read runs in a worker, while the calling thread uses bounded waits to dispatch Ctrl+C. Interrupting a call kills and reaps only its CLI and propagates `KeyboardInterrupt`; it does not stop the Host or implicitly cancel the request. SDK CLI processes run without a console on Windows so Ctrl+C is handled by the Python caller. The interactive `popu` / `python -m popu` entry point instead preserves the terminal console and standard streams, while using a separate native process group so Python remains responsible for Ctrl+C cleanup.

`Client(timeout=2)` optionally limits each synchronous CLI operation to two seconds, including reading its response and waiting for process exit. The default is no deadline. Expiry raises `PopuError` and cleans up the CLI. A dialog's submission deadline ends at acceptance; it does not limit the later background reader or replace `timeout_seconds`. An explicitly configured client deadline also applies to a foreground `wait` call.

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

## Runtime and source development

For source development, build and install the development environment from the repository root (currently Windows only):

```powershell
just setup-python
```

This recipe builds the debug CLI/Host, stages it into `python/src/popu/_native` with `--dev`, then runs `pip install -e ./python`. You can then use `Client()`, `popu`, and the packaged demos directly. Rerun the recipe after changing the CLI/Host. Staging resolves paths through Cargo metadata and respects `CARGO_TARGET_DIR`. Running `pip install -e ./python` alone does not build or stage a native executable.

Use `Client(executable="path/to/popu.exe")` to select another local build. A command sequence such as `["path/to/popu.exe"]` is also accepted. The SDK targets the current Host protocol and does not accept pre-Host response formats.

The product version is maintained only in `src-tauri/Cargo.toml`. Builds and editable installs generate `_version.py` automatically; the installed SDK is self-contained. `just test-python` also prepares the version before running tests. For an uninstalled source checkout on `PYTHONPATH`, run `just prep-python` first. Build metadata requires the complete checkout; users should install the Windows wheel.

By default, the CLI and SDK verify the bundled payload and copy it to `%LOCALAPPDATA%\Popu\runtime\<version>-win_amd64-<manifest-hash>\`. Native processes run from this immutable cache, so an active Host does not lock files inside `site-packages` during upgrade or uninstall. Old caches are retained; automatic cleanup is not implemented.

Before a business request, the native CLI and Host require matching application and RPC versions through a handshake on the same connection. A legacy or incompatible Host produces a diagnostic before submission; Python raises `PopuError`. Inspect it with `python -m popu host status`, then explicitly stop it with `python -m popu host stop` or exit from its tray menu when ready. Popu never automatically stops an existing Host. Management commands remain available without the business handshake. Handshakes have a two-second deadline; this does not limit a dialog's business wait. A submitted business request is not automatically replayed after a transport failure.

Before starting a new Host, the CLI checks WebView2 availability. Missing/unavailable runtimes produce a manual installation link and the loader diagnostic rather than a background startup timeout. This check does not install software or open the browser. See [build and publishing (Chinese)](https://github.com/ZeroKwok/Popu/blob/HEAD/docs/python-distribution.md) for package verification and release procedures.
