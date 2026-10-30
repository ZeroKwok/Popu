# Popu

> Local notification management and lightweight interaction windows for automation scripts and AI agents.

[中文版](README.md)

Popu lets scripts and agents notify people, request confirmation, collect small amounts of structured input, and preserve traceable notification history. It runs locally without a network service. The CLI and Python SDK submit requests to one persistent Host that owns windows, queues, request state, history, and tray reminders.

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
- local Named Pipe on Windows or Unix Domain Socket on Unix;
- no network listener and no external service required.

## Quick start

Run the following commands from the repository root. First follow [Build and test](#build-and-test) to build the `popu` executable and add its directory to `PATH`.

### CLI

Submit a notification:

```powershell
popu request protocol/examples/notification-success.json
```

Submit a confirmation dialog:

```powershell
popu request protocol/examples/dialog-confirm.json
```

Requests are JSON files. See the [JSON Schema](protocol/popu.schema.json) and [architecture documentation](ARCHITECTURE.md) for the complete protocol.

### Python SDK

Python 3.10+ is required. Make sure the `popu` executable is available on `PATH`:

```powershell
python -m pip install -e ./python
```

Send a notification:

```python
from popu import Client, Notification

client = Client()
receipt = client.notify(
    Notification.success(
        title="Backup completed",
        message="All files were backed up successfully.",
    )
)

print(receipt.request_id)
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

if result.status == "submitted":
    print("confirmed")
```

See [python/README.md](python/README.md) for the complete Python SDK guide.

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

Long-running notifications opt in with `updatable: true` and reuse one ID. The last update sets `updatable: false`; it may request `reshow: true` and a fresh `duration_ms`. User closes silence ordinary updates, with one explicit reshow allowed by default; automatic expiry allows the next report to show again. Live records expire after 12 hours without a successful report. See [live notifications](docs/live-notifications.md) and the [Python backup demo](python/examples/README.md).

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
- [Python SDK](python/README.md)
- [JSON Schema](protocol/popu.schema.json)
- [Contributing](CONTRIBUTING.md)
- [Security](SECURITY.md)

## Build and test

The project is currently validated primarily on Windows 10/11 with Rust stable, Node.js 20+, and the WebView2 Runtime. The protocol and IPC design include a Unix Domain Socket path; window capabilities depend on the target platform.

Install frontend dependencies and build:

```powershell
npm ci
npm run build
```

On Windows, the executable is `src-tauri/target/release/popu.exe`. Add that directory to `PATH` or invoke the executable by its path. Python can also use `Client(executable="src-tauri/target/release/popu.exe")`. The Python SDK is a CLI client; installing it does not install the Rust executable.

Run tests:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
node --check ui/app.js
python -m pytest python/tests -q
```

During development, build the UI and Rust executable separately:

```powershell
npm run build:ui
cargo build --manifest-path src-tauri/Cargo.toml
```

Issues and pull requests are welcome; see [Contributing](CONTRIBUTING.md). Native notification lifecycle tests are documented in [notification window coordination](docs/notification-layout.md) and require an unlocked Windows desktop.

## Scope

Popu is a local human-interaction layer. It is not a task scheduler, logging system, remote push service, or full business application UI. Callers own task execution and business data; Popu reliably presents the summaries and decisions that require a person's attention.

The project is evolving. Unless explicitly requested, CLI, SDK, and protocol interfaces do not promise backward compatibility.

## License

Apache License 2.0. See [LICENSE](LICENSE).
