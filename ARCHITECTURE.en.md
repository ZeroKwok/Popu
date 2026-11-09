# Popu Host architecture

[简体中文](ARCHITECTURE.md)

Popu is a single-instance persistent Tauri Host for bounded local human interactions. CLI and Python processes are clients; they never own notification lifetime or create their own WebView process.

The repository is coordinated by root just tasks. `ui/` is an independent npm project: static source assets live in `ui/src/`, chart bundle entries in `ui/charts/`, and generated distributable assets in `ui/dist/`. Its dependencies and browser tests stay outside the embedded resource tree. The Cargo project in `src-tauri/` consumes only that UI distribution; the Python project in `python/` packages the SDK and staged native executable. Root `scripts/` contains cross-project build and native verification helpers.

`src-tauri/Cargo.toml`'s `package.version` is the sole product release version. Tauri inherits it, and Python build metadata/editable installs generate the ignored SDK `_version.py` before packaging. Installed wheels contain this generated module and never read a repository Cargo manifest. Protocol/RPC versions remain separate compatibility identifiers.

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

- The first client starts hidden `popu host run` and retries the connection. Listener binding is the single-instance boundary. The Host accepts and queues requests during initialization. The coordinator starts native window work after Tauri delivers `RunEvent::Ready` at the end of setup, including any pending history prewarm.
- On Windows, `host_process.rs` launches the Host detached with `CreateProcessW(..., bInheritHandles = FALSE, ...)`. The Host inherits neither the submitting CLI's standard streams nor unrelated inheritable handles. Redirecting standard streams alone does not provide this isolation. Unix launches retain null standard streams through `Command`.
- The Host generates UUID request IDs and starts every request at revision 1.
- Dialogs are globally FIFO with one active window. A dialog CLI waits for a terminal response and adopts its action exit code.
- Notifications use the primary display, never wait behind dialogs, show at most three, and remain alive after the submitting client exits. `duration_ms` starts when the window is presented and enters `shown`.
- Notification windows are always on top without requesting focus. Omitting `window.always_on_top` defaults to `true`; explicitly setting it to `false` is rejected by request validation and JSON Schema. Dialogs may still set it to `false`.
- Closing or expiring a visible request promotes queued work and reflows the notification stack.
- Host shutdown terminalizes ordinary requests and wakes waiters. Updatable notifications retain durable checkpoints and restore hidden on the next start, preserving their idle deadline and user intent.

`RequestRegistry` tracks the request state. The coordinator advances window preparation and presentation through these main transitions.

```text
new --> queued --> preparing --> ready --> shown --> terminal
                       ^           |         |
                       +-----------+         +----> hidden
                         newer revision                |
           queued <------------------------------------+
                         eligible update / reshow

shown --> ready     temporarily parked after a display change
```

`queued` waits for Host startup or scheduling. `preparing` creates the window and waits for native preparation and the current revision, with a preparation deadline. `ready` means preparation is complete and the window may still wait for screen space. `shown` means the window has been presented. An updatable notification can enter `hidden` after closing or timing out, then return to the queue on an eligible update or `reshow`. Dismissal or preparation failure can end a request before presentation. Terminal states are `confirmed`, `cancelled`, `closed`, `expired`, `failed`, and `stale`.

## Window sizing and DPI

Request `window.width` and `window.height`, including the dialog and notification defaults, are logical pixels. Clients must use the same values on 1080p, 2K, and 4K displays; they must not multiply dimensions by display resolution or Windows scaling.

Default widths are `360` for notifications and `440` for dialogs. An omitted height fits the initial whole-window layout within `120–480` or `160–720`, respectively, capped by the target area minus 16 logical pixels at each vertical edge. The size locks at first presentation; later content changes use internal scrolling. Images reserve a fixed box before decoding (default 320); charts use a fixed height (default 220). Explicit window heights keep their existing semantics. External templates without measurement support use a bounded fallback height of 220 or 300. See [initial content sizing](docs/window-sizing.md) for the implementation contract.

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

On Windows, notifications and dialogs use transparent, frameless native windows with cards rendered by the WebView. Initial sizing and frame configuration happen while the window is hidden. `runtime.rs` then installs the message handler asynchronously on the thread that owns the HWND and reports the result to the coordinator. After installation, the coordinator lets the window render under DWM cloaking. Both native installation and the current revision's rendering acknowledgement must complete before `ready`, within the same preparation deadline. Tao can write native frame styles again when showing or hiding the window, so the handler keeps those bits clear during `WM_STYLECHANGING`. It removes itself on `WM_NCDESTROY`.

Opening a WebView context menu can activate the window and send `WM_NCACTIVATE`. Default processing can repaint a stale caption in the transparent margin even with the caption style cleared. The handler forwards the activation state with `lParam = -1` so Tao updates focus and Win32 skips the non-client repaint. The WebView handles its context menu. The native lifecycle regression checks style bits after the first show and after re-showing. Comparing screenshots around activation and right-click verifies the separate repaint behavior. [Win32 activation message](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-ncactivate)

## Identity, coalescing, and updates

`coalesce_key` is an optional live business-slot key scoped by mode. A duplicate create returns the existing ID, state, and revision without changing content. Terminalization releases the key immediately.

Updates contain `request_id`, `expected_revision`, and a complete `content` snapshot. Success increments the revision and refreshes the occurrence time. Notifications opt in at creation with `updatable: true` (default false); the final update sets it false. Update omission preserves the current flag. Every accepted notification update restarts the display timer using the configured duration; optional `duration_ms` overrides that duration. Queued/preparing/parked notifications receive the full duration when actually shown. Invalid updates, revision conflicts and repeated readiness acknowledgements do not extend timers. Dialog updates retain their existing form-preserving behavior and original interaction deadline, and reject notification options.

Automatic display expiry hides an updatable notification; the next report queues it again. User close hides it and silences ordinary reports. An explicit `reshow: true` may override user close once by default, counted only on actual presentation. Closing the update channel is independent of displaying its final contents. Twelve hours without an accepted report ends it as `stale`. Full behavior and examples are in [live notifications (Chinese)](docs/live-notifications.md).

External templates are caller configuration supplied with `--template`. The Host canonicalizes and validates the directory before registering the request.

Content acceptance and presentation are tracked separately. The Host advances its internal presented revision only on current-version `ui_ready` for a shown window, or when a ready window is actually revealed; checkpoints do not restore it. UI close events carry the painted revision at the time of the action. Native close events capture the Host's presented revision when received, so delayed closes cannot acknowledge later content. Dialog actions must match the latest presented revision; conflicts keep the window and input intact and ask the user to review again. Actions are disabled while replacement content is preparing. Closing without an action, or cancelling with Esc, can still end a dialog.

Forms submit through Popu actions and prevent browser navigation. Enter in a text or number input submits only when exactly one action has `submit: true`, validating all form blocks; multiple submit actions require an explicit button choice. Enter in a textarea retains a newline. Notifications do not support Esc dismissal; Esc cancels dialogs.

## RPC

Every frame is capped at 1 MiB. RPC version `1.0` methods are:

- `request.create`, `request.update`, `request.dismiss`, `request.get`, `request.wait`
- `host.hello`
- `host.status`, `host.stop`
- `history.list`, `history.get`, `history.clear`

Malformed JSON, unsupported versions, unknown methods, oversized frames, revision conflicts, terminal updates, and missing IDs return structured errors.

Currently, `persistence_failed` does not guarantee that nothing happened: the Registry or window may already have changed, and the Host retries saving in the background. With a known ID, reconcile revision and content through `status`. If creation returned no ID, repeating a request with its original `coalesce_key` can reuse only a still-active request. Without that key, or after the original request ends, retrying is not guaranteed to be idempotent; do not blindly replay it.

Each business connection begins with `host.hello` (`params.app_version`). The Host validates the CLI application version and the frame's RPC version, then returns `app_version`, `rpc_version`, `pid`, and `executable`. The client validates that identity before sending one business request on the same connection. Both versions must match exactly. A two-second handshake deadline uses nonblocking I/O, including partial frames, without limiting a later business wait. Hosts reject business calls without a handshake. `host.status` and explicitly requested `host.stop` remain available without it for upgrade recovery; status includes the running identity under `host`.

Only connection absence/refusal triggers automatic Host startup. Handshake failures and business transport errors never replay a request or replace an existing Host. On Windows, WebView2 availability is checked via its loader before detached startup and before direct `host run`; failure reports a manual installation link without launching an installer. The Host and Python distribution details are in [Python distribution (Chinese)](docs/python-distribution.md).

## History

Notification history and live notification checkpoints are stored in SQLite under the Popu application-data directory. Accepted updates replace the existing history row, keeping content, revision, occurrence time and reading state consistent. Checkpoints retain the request, template location, user-close intent and reshow budget. History and checkpoint changes commit atomically before successful command replies. Dialogs and form values are never persisted. Retention keeps the newest 500 archived rows and preserves active notifications.

The 500-row limit applies to database archives, not the Host's in-memory request count; terminal Registry records currently have no automatic eviction. `status` / `wait` read the current Host Registry; the history window and `history.*` read SQLite. Restart restores remaining active notification checkpoints, not archived IDs or dialogs. A retained history entry therefore does not guarantee that its old ID is queryable through `status` / `wait`. Clearing hidden history removes its update channel; later queries or updates return a missing-ID error.

The history window is a read-only master-detail view of current and archived notification snapshots. Its list shows title, presentation state and occurrence time; its detail pane renders the saved body. Viewing it never starts a timer, changes the stack or emits a business action. Clearing history also deletes hidden update channels, while preserving queued/preparing/ready/shown notifications. The coordinator serializes clearing with updates.

When a notification snapshot is successfully recorded, or history is cleared, the Host emits a targeted `history-changed` event. The history window coalesces refresh hints and preserves the selected item when possible. No window polls the database, and an unopened history window receives no work.

Reading is per content revision. User close acknowledges the displayed revision, while later content starts unread. The foreground history renderer acknowledges only the revision it actually rendered; a late acknowledgement cannot read a newer result. Tray reminders depend only on hidden/archived unread rows with `requires_read: true`, never on visible or queued counts.

The snapshot's `acknowledged_revision` is maintained internally by the Host for close or caller-dismissal acknowledgements; it is not a request input. History viewing records reading separately with revision checks. Queued one-shot notifications generally do not appear in history before terminalization. Continued updates restart visible notification timers, and no maximum queue waiting time is currently guaranteed.

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
- `ipc/handshake.rs`: Host identity negotiation and the business connection handshake.
- `host_client.rs`: connection acquisition and startup, without business request replay.
- `preflight.rs`: WebView2 availability diagnostics before Host startup.
- `host.rs`: RPC dispatch, synchronization, multi-window ownership, and UI commands.
- `history.rs`: notification-only SQLite persistence and retention.
- `runtime.rs`: target-monitor/DPI discovery, physical placement, and frameless-window helpers.
- `placement.rs`: DPI-independent logical sizing plus logical-to-physical placement conversion.
- `protocol.rs`: versioned request/response validation and logical window-dimension semantics.

The Python SDK invokes the public CLI and exposes create/update/dismiss/status/wait. It does not implement a second server or a notification lifetime pipe.

The SDK requires Python 3.10+ and the current Host CLI. Acceptance receipts are parsed strictly; pre-Host response compatibility is not provided. A shared CLI invocation wrapper owns the stdout pipe, stderr temporary file and process cleanup. Notifications and management read one UTF-8 JSON line before waiting for the CLI to exit, without draining trailing stdout to EOF. There is no stdout spool file or receipt polling. A worker performs each blocking response read; bounded queue/process waits on the calling thread allow Windows Ctrl+C dispatch and optional per-operation client deadlines. Interrupts and deadlines kill/reap the CLI before joining its reader and closing streams. Windows SDK CLIs run without a console, and the native Host launcher isolates inherited handles. After a dialog is accepted, its foreground deadline is cleared and a background reader drains remaining stdout in bounded chunks and reaps the CLI. Every SDK `wait` invokes the public `wait` command and returns the full Host snapshot, so results do not depend on which Python client submitted the request. Business action exit codes, including 12, are read from that snapshot rather than interpreted as transport errors. Temporary request/update files are removed after use, interruption or submission failure.

Python notification builders explicitly emit topmost windows on the primary work area; dialog builders retain configurable topmost/display/area/size options. Both builders expose content alignment and live coalescing keys. Complete request mappings remain available for forms, arbitrary severities, and custom actions.
