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
- States are `queued`, `shown`, `confirmed`, `cancelled`, `closed`, `expired`, and `failed`.
- Dialogs are globally FIFO with one active window. A dialog CLI waits for a terminal response and adopts its action exit code.
- Notifications use the primary display, never wait behind dialogs, show at most three, and remain alive after the submitting client exits. `duration_ms` starts at `ui_ready`.
- Closing or expiring a visible request promotes queued work and reflows the notification stack.
- Host shutdown terminalizes active requests, wakes waiters, destroys windows, and exits the process.

## Identity, coalescing, and updates

`coalesce_key` is an optional live business-slot key scoped by mode. A duplicate create returns the existing ID, state, and revision without changing content. Terminalization releases the key immediately.

Updates contain `request_id`, `expected_revision`, and a complete `content` snapshot. Success increments the revision. A stale update returns `revision_conflict` with the current revision. Updates do not restart timers. A shown dialog's form-block structure is immutable; the renderer preserves current input while applying safe content updates.

External templates are caller configuration supplied with `--template`. The Host canonicalizes and validates the directory before registering the request.

## RPC

Every frame is capped at 1 MiB. RPC version `1.0` methods are:

- `request.create`, `request.update`, `request.dismiss`, `request.get`, `request.wait`
- `host.status`, `host.stop`
- `history.list`, `history.clear`

Malformed JSON, unsupported versions, unknown methods, oversized frames, revision conflicts, terminal updates, and missing IDs return structured errors.

## History

Only terminal notifications are stored in SQLite under the Popu application-data directory. Records contain the final content snapshot, revision, timestamps, terminal state, and optional response. Dialogs and form values are never persisted. Insertion prunes history to the newest 500 rows using a stable timestamp/row sequence.

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
- `runtime.rs`: thin platform window-placement and frameless-window helpers.
- `protocol.rs`: versioned request/response data validation.

The Python SDK invokes the public CLI and exposes create/update/dismiss/status/wait. It does not implement a second server or a notification lifetime pipe.
