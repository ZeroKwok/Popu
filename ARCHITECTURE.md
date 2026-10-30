# Popu Host architecture

Popu is a single-instance persistent Tauri Host for bounded local human interactions. CLI and Python processes are clients; they never own notification lifetime or create their own WebView process.

```text
CLI / Python SDK
       │ 4-byte big-endian length + UTF-8 JSON RPC
       ▼
interprocess::local_socket
       │ Windows Named Pipe / Unix Domain Socket
       ▼
Tauri Host ── RequestRegistry ── request_id → WebviewWindow
                 ├─ one active dialog + global FIFO
                 └─ three visible notifications + global FIFO
```

## Lifecycle and scheduling

- The first client starts hidden `popu host run` and retries readiness. Listener binding is the single-instance boundary.
- The Host generates UUID request IDs and starts every request at revision 1.
- States are `queued`, `preparing`, `ready`, `shown`, `hidden`, `confirmed`, `cancelled`, `closed`, `expired`, `failed`, and `stale`. `hidden` is nonterminal and retains the update channel.
- Dialogs are globally FIFO with one active window. A dialog CLI waits for a terminal response and adopts its action exit code.
- Notifications use the primary display, never wait behind dialogs, show at most three, and remain alive after the submitting client exits. `duration_ms` starts at `ui_ready`.
- Notification windows are always on top without requesting focus. Omitting `window.always_on_top` defaults to `true`; explicitly setting it to `false` is rejected by request validation and JSON Schema. Dialogs may still set it to `false`.
- Closing or expiring a visible request promotes queued work and reflows the notification stack.
- Host shutdown terminalizes ordinary requests and wakes waiters. Updatable notifications retain durable checkpoints and restore hidden on the next start, preserving their idle deadline and user intent.

## Window sizing and DPI

Request `window.width` and `window.height`, including the dialog and notification defaults, are logical pixels. Clients must use the same values on 1080p, 2K, and 4K displays; they must not multiply dimensions by display resolution or Windows scaling.

On Windows, placement keeps two coordinate spaces deliberately separate:

```text
request logical size + logical margin
             │
             ▼
protocol / placement ── target monitor effective DPI ──► physical size + physical position
             │                                                  │
             ▼                                                  ▼
WebView logical CSS viewport                         Win32 work area / set_position
```

- `runtime.rs` selects the target monitor, reads its effective DPI, and obtains its work area in physical pixels.
- `placement.rs` clamps logical request dimensions against the target logical work area, then derives the physical native size, margin, and position from that monitor's scale factor.
- `host.rs` first moves a hidden window onto the target monitor, sets its logical WebView size, and applies the computed physical position. This prevents a high-DPI display from shrinking the CSS viewport or clipping a right-aligned window.
- Notification stack offsets are physical values derived from the same placement, so spacing remains visually consistent at different DPI scales. A stack is reflowed when a notification leaves it; periodic timer ticks and content-only updates do not overwrite a user's drag.

`"100%" × "100%"` continues to fill the selected physical display area after conversion. The resulting logical viewport varies with DPI, as expected for a fullscreen window.

## Identity, coalescing, and updates

`coalesce_key` is an optional live business-slot key scoped by mode. A duplicate create returns the existing ID, state, and revision without changing content. Terminalization releases the key immediately.

Updates contain `request_id`, `expected_revision`, and a complete `content` snapshot. Success increments the revision and refreshes the occurrence time. Notifications opt in at creation with `updatable: true` (default false); the final update sets it false. Update omission preserves the current flag. Updates optionally carry `reshow` and `duration_ms`; an explicit duration restarts the current display timer. Dialog updates retain their existing form-preserving behavior and reject notification options.

Automatic display expiry hides an updatable notification; the next report queues it again. User close hides it and silences ordinary reports. An explicit `reshow: true` may override user close once by default, counted only on actual presentation. Closing the update channel is independent of displaying its final contents. Twelve hours without an accepted report ends it as `stale`. Full behavior and examples are in [live notifications](docs/live-notifications.md).

External templates are caller configuration supplied with `--template`. The Host canonicalizes and validates the directory before registering the request.

## RPC

Every frame is capped at 1 MiB. RPC version `1.0` methods are:

- `request.create`, `request.update`, `request.dismiss`, `request.get`, `request.wait`
- `host.status`, `host.stop`
- `history.list`, `history.clear`

Malformed JSON, unsupported versions, unknown methods, oversized frames, revision conflicts, terminal updates, and missing IDs return structured errors.

## History

Notification history and live notification checkpoints are stored in SQLite under the Popu application-data directory. Accepted updates replace the existing history row, keeping content, revision, occurrence time and reading state consistent. Checkpoints retain the request, template location, user-close intent and reshow budget. History and checkpoint changes commit atomically before successful command replies. Dialogs and form values are never persisted. Retention keeps the newest 500 archived rows and preserves active notifications.

The history window is a read-only master-detail view of current and archived notification snapshots. Its list shows title, presentation state and occurrence time; its detail pane renders the saved body. Viewing it never starts a timer, changes the stack or emits a business action. Clearing history also deletes hidden update channels, while preserving queued/preparing/ready/shown notifications. The coordinator serializes clearing with updates.

When a notification snapshot is successfully recorded, or history is cleared, the Host emits a targeted `history-changed` event. The history window coalesces refresh hints and preserves the selected item when possible. No window polls the database, and an unopened history window receives no work.

Reading is per content revision. User close acknowledges the displayed revision, while later content starts unread. The foreground history renderer acknowledges only the revision it actually rendered; a late acknowledgement cannot read a newer result. Tray reminders depend only on hidden/archived unread rows with `requires_read: true`, never on visible or queued counts.

## Public CLI

```text
popu request <file|-> [--template <trusted-directory>]
popu update <request-id> <update.json>
popu dismiss <request-id>
popu status <request-id>
popu wait <request-id>
popu host status
popu host stop
```

The internal `popu host run` command is hidden. Exit codes are 0 for success/confirmation, 10 for cancellation/close, 11 for timeout, and 12 for validation/runtime errors; an action may override its dialog client's code with 0–255 without terminating the Host.

## Source boundaries

- `core.rs`: Tauri-independent registry, states, FIFO scheduling, coalescing, revision checks, and timers.
- `ipc.rs`: bounded framing and local-socket client/listener construction.
- `host.rs`: RPC dispatch, synchronization, multi-window ownership, and UI commands.
- `history.rs`: notification-only SQLite persistence and retention.
- `runtime.rs`: target-monitor/DPI discovery, physical placement, and frameless-window helpers.
- `placement.rs`: DPI-independent logical sizing plus logical-to-physical placement conversion.
- `protocol.rs`: versioned request/response validation and logical window-dimension semantics.

The Python SDK invokes the public CLI and exposes create/update/dismiss/status/wait. It does not implement a second server or a notification lifetime pipe.

The SDK requires Python 3.10+ and the current Host CLI. Acceptance receipts are parsed strictly; pre-Host response compatibility is not provided. After a dialog is accepted, a background reader drains and reaps its CLI subprocess. Every SDK `wait` invokes the public `wait` command and returns the full Host snapshot, so results do not depend on which Python client submitted the request. Business action exit codes, including 12, are read from that snapshot rather than interpreted as transport errors. CLI streams use UTF-8, and temporary request/update files are removed after use or submission failure.

Python notification builders explicitly emit topmost windows on the primary work area; dialog builders retain configurable topmost/display/area/size options. Both builders expose content alignment and live coalescing keys. Complete request mappings remain available for forms, arbitrary severities, and custom actions.
