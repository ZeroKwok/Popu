# Popu

> Local notification management and lightweight interaction windows for automation scripts and AI agents.

[中文版](README.md)

Popu lets scripts and agents notify people, request confirmation, collect small amounts of structured input, and preserve traceable notification history. It runs locally without a network service. One pip installation provides both the Python SDK and the `popu` command-line tool.

## Quick start

### Install and run the demo

**Currently supported: Windows x64 with 64-bit Python 3.10+ and [Microsoft WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/).** If WebView2 is missing, Popu provides a manual installation link.

```sh
python -m pip install popu
python -m popu.demo progress
```

The demo shows task progress followed by a result summary. Try closing the progress card to see background updates and the final summary's explicit reshow. It uses simulated backup data without accessing your files. Press Ctrl+C to stop it. Add `--wait` to wait for the final notification to finish displaying. Console messages are in English.

The package includes the matching native executable. Run it from any directory without a repository checkout or build tools. The background Host starts automatically on the first request.

### Python SDK

Send a notification:

```python
from popu import Client, Notification

Client().notify(
    Notification.success(
        title="Backup completed",
        message="All files were backed up successfully.",
    )
)
```

Ask for confirmation:

```python
from popu import Client, DialogRequest

result = Client().show(
    DialogRequest.confirm(
        title="Deploy release?",
        message="Production deployment is ready.",
    )
)

if result.status == "submitted" and result.action == "confirm":
    print("Deployment confirmed")
```

See the [Python quick start](python/README.md#update-task-progress) for progress updates and the [SDK reference](docs/python-sdk.md) for complete behavior.

### CLI

```sh
popu --help
popu templates list
```

Save the following as `notification.json` to submit a notification from any directory:

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

If `popu` is not on `PATH`, use `python -m popu` instead. See the [JSON Schema](protocol/popu.schema.json) and [architecture documentation](ARCHITECTURE.md) for other requests.

## Why Popu

Automation often needs to tell a person what happened or wait for an explicit decision before continuing. Popu is useful for:

- reporting completion of backups, builds, deployments, and other jobs;
- asking for confirmation before an AI agent performs a risky action;
- showing progress, key facts, and a concise result summary for long-running work;
- managing notifications from multiple scripts in one place;
- keeping timed-out or completed notifications discoverable in history after the submitting process exits.

## Core capabilities

### Notification management

- one persistent Host owns notification lifetimes;
- strict FIFO scheduling with up to three visible notifications on the primary work area;
- acceptance receipts, status queries, complete content updates, dismissal, and terminal waiting;
- `coalesce_key` support to avoid duplicate requests for the same business slot;
- notification history, unread state, and tray reminders;
- notification windows are always on top without actively stealing focus;
- notifications remain managed by the Host after the submitting CLI or Python process exits.

### Lightweight interaction

- `dialog` confirmation windows and small forms;
- structured actions, form values, terminal states, and exit codes;
- paragraphs, properties, lists, progress, images, and Vega-Lite charts;
- content updates with revision conflict detection;
- frameless, transparent local windows with request-configured dimensions.

### Integration

- JSON CLI;
- Python SDK;
- local IPC through one background Host for windows and history;
- no network listener and no external service required.

## How it works

```text
CLI / Python SDK
        │
        ▼
Persistent Popu Host
        │
        ├── notification queue
        ├── dialog queue
        ├── request registry
        ├── notification history
        └── tray reminder
```

The Host starts automatically when the first request arrives. Callers receive a Host-generated `request_id` and revision:

| CLI entry point or request mode | Behavior |
| --- | --- |
| `popu request <file>`, `mode: "notify"` | Return an acceptance receipt; the Host continues managing the notification |
| `popu request <file>`, `mode: "dialog"` | Print an acceptance receipt, then wait and print the response for confirmation, cancellation, close, timeout, or another terminal outcome |
| `popu update <id> <update.json>` | Replace the complete `content` snapshot using the latest revision |
| `popu dismiss <id>` | Cancel a queued or visible request |
| `popu status <id>` | Read the current complete request snapshot |
| `popu wait <id>` | Wait for a terminal state and read the Host snapshot |

`duration_ms` starts after notification content is ready and the native window is actually shown. Dialog terminal states and structured responses do not depend on the submitting process remaining alive; notifications also continue after that process exits.

Terminal notifications are saved to history; dialogs and their form values are not persisted. Only unread history entries with `requires_read: true` keep the tray reminder active.

Long-running notifications opt in with `updatable: true` and reuse one ID. Every accepted update restarts the display timer using the configured duration; `duration_ms` can override it. The last update sets `updatable: false` and may request `reshow: true`. User closes silence ordinary updates, with one explicit reshow allowed by default; automatic expiry allows the next report to show again. Live records expire after 12 hours without a successful report. See [live notifications](docs/live-notifications.md) and the [Python progress demo](python/src/popu/demo/README.md).

## Notification and window rules

- notifications use the primary display work area, with at most three visible at once;
- notification windows always use `always_on_top: true`; an explicit `false` is rejected;
- notifications do not normally take focus;
- dialogs can configure their display, work area or physical screen, size, and topmost behavior independently;
- width and height use logical pixels or percentages, without manual DPI conversion;
- both notifications and dialogs support selectable text, images, and charts;
- large logs, complete reports, and complex editing belong in the caller's product; Popu presents summaries and necessary input.

## Documentation

- [中文版 README](README.md)
- [Architecture](ARCHITECTURE.md)
- [Notification window coordination and layout](docs/notification-layout.md)
- [Python installation and quick start](python/README.md)
- [Python SDK reference](docs/python-sdk.md)
- [Live notification lifecycle](docs/live-notifications.md)
- [Python package build and publishing](docs/python-distribution.md)
- [JSON Schema](protocol/popu.schema.json)
- [Contributing](CONTRIBUTING.md)
- [Security](SECURITY.md)

## Build and test

The project is currently validated primarily on Windows 10/11 with Rust stable, Node.js 20+, just, and the WebView2 Runtime. Root just tasks coordinate the npm project in `ui/`, Cargo project in `src-tauri/`, and SDK/wheel project in `python/`. UI sources live in `ui/src/`; Cargo embeds only `ui/dist/` build output. The protocol and IPC design include a Unix Domain Socket path; window capabilities depend on the target platform.

For a fresh checkout, install ImageMagick. Install frontend dependencies, prepare generated inputs, and build:

```powershell
just setup
just build-release
```

On Windows, the executable is `src-tauri/target/release/popu.exe`. Add that directory to `PATH` or invoke it by path. For source development, run `python -m pip install -e ./python` and use `Client(executable="src-tauri/target/release/popu.exe")`; editable installs do not build or download binaries. The Windows wheel includes both the native executable and SDK.

Run tests:

```powershell
just check
# Browser visual regression (does not start the native Host):
just test-visuals
```

During development, build the UI and Rust executable separately:

```powershell
just build-ui
cargo build --manifest-path src-tauri/Cargo.toml
```

Change only `package.version` in `src-tauri/Cargo.toml` for a product release. Tauri inherits it; Python builds and editable installs generate the SDK version automatically. `just version` prints it; `just build-wheel` builds the bundled local wheel. Install pytest and build in your development virtual environment for Python tests and wheel builds.

Running `just` displays grouped help. Use `just check` for automated checks; use `just demo-fixture notification-success.json`, `just demo-fixtures notification`, or `just demo-python --wait` for interactive demos. Demos and native wheel staging resolve executables through Cargo metadata, including `CARGO_TARGET_DIR`. Set `PYTHON` to select an interpreter. Builds prepare missing/outdated icons automatically; `just icons` forces regeneration. The current `build-wheel` and `test-native` tasks are Windows-only.

Issues and pull requests are welcome; see [Contributing](CONTRIBUTING.md). Native notification lifecycle tests are documented in [notification window coordination](docs/notification-layout.md) and require an unlocked Windows desktop.

## Scope

Popu is a local human-interaction layer. It is not a task scheduler, logging system, remote push service, or full business application UI. Callers own task execution and business data; Popu reliably presents the summaries and decisions that require a person's attention.

The project is evolving. Unless explicitly requested, CLI, SDK, and protocol interfaces do not promise backward compatibility.

## License

Apache License 2.0. See [LICENSE](LICENSE).
