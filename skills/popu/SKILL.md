---
name: popu
description: Design and invoke concise Popu notifications, live task progress, and lightweight dialogs through its Python SDK or JSON CLI. Use when automation or an AI agent needs to report an outcome, keep a task summary current, request a decision, or collect a small amount of structured input.
---

# Popu interaction design

Popu is a local interaction layer with a persistent Host. The caller owns task execution and business state; Popu owns windows, queues, notification history, and update channels. Notification presentation states do not indicate whether the caller's task succeeded or failed.

## Choose the interaction

| Need | Use |
| --- | --- |
| The task can continue without a meaningful user-facing update | Do not show a window. |
| A result or noteworthy outcome needs no reply | A one-shot notification. |
| A long-running task has progress, key facts, and a final summary | One notification created with `updatable: true`, updated through the same ID. |
| The task needs a decision, acknowledgement, choice, or small structured value | A dialog. |
| The task needs a long log, complex editing, file picking, authentication, or a full application view | Summarize and hand off to the owning product or workflow. |

Use one dialog for one decision. A progress notification reports the caller's task; it does not run or control that task.

## Invoke the installed tool

- Use the Python SDK for Python integrations or the JSON CLI for other callers. Respect the user's chosen interface.
- When installation is needed and in scope, use `python -m pip install popu`. The package includes the matching native executable and SDK. It currently supports Windows x64 and 64-bit Python 3.10+; Microsoft WebView2 Runtime is required and is installed manually if missing.
- Both `popu` and `python -m popu` expose the CLI. Use `--version`, `--help`, and `templates list` to inspect an installation without creating a request. The Host starts automatically on the first business request; do not start a second Host manually.
- `Client()` resolves the bundled version rather than searching `PATH`. Use `Client(executable=...)` only for a caller-selected local build or deployment override.
- An incompatible running Host is rejected before submission. Inspect `host status`; stop or exit the old Host only when ending its active interactions is appropriate and authorized by the task. Do not replace it as an automatic retry.
- `python -m popu.demo progress` demonstrates a simulated task without modifying user files. Add `--wait` to wait for the final presentation to end. Keep demo console messages in English; choose actual notification text for the user's language.

Read [integration examples](references/examples.md) when implementing SDK calls, raw requests, progress updates, or visuals. For nontrivial raw JSON, consult the authoritative [request schema](https://github.com/ZeroKwok/Popu/blob/master/protocol/popu.schema.json) and [update schema](https://github.com/ZeroKwok/Popu/blob/master/protocol/update.schema.json); in a source checkout, use the local `protocol/` files and examples.

## Handle acceptance and results

### CLI

Write a request to a caller-controlled file, then invoke `popu request <request.json>`; `popu request -` accepts UTF-8 JSON on stdin. Read stdout as JSON lines:

- The first line is an acceptance receipt with `status: "accepted"`, `request_id`, `state`, `revision`, and `duplicate`. Acceptance may mean queued, not visibly shown.
- A notification CLI returns after acceptance. A dialog CLI continues waiting and emits a later terminal response with `status`, `action`, and `values`. Do not parse dialog stdout as one JSON document or mistake the receipt for a decision.
- `status <id>`, `update <id> <update.json>`, `dismiss <id>`, and `wait <id>` return Host snapshots. A wait snapshot includes the terminal response and business exit code when present.
- Exit codes 10 and 11 normally represent cancellation/close and timeout; 12 normally represents validation/runtime errors. Dialog actions may override their business exit code with any value from 0-255, including 12. Interpret a terminal response as well as the process code, and inspect stderr for invocation failures.

### Python SDK

`Client.notify()` and `Client.create()` return a `RequestReceipt`; `create()` also returns after accepting a dialog. `Client.show()` waits for a dialog and returns `InteractionResult`. `status()`, `update()`, `dismiss()`, and `wait()` return snapshot dictionaries; `wait()` is not an `InteractionResult`.

Branch on both the terminal status and intended action before executing the user's decision. Treat cancelled, closed, and timed-out dialogs as expected outcomes. `PopuError` reports invocation, validation, compatibility, or protocol failures. Ctrl+C propagates `KeyboardInterrupt` and reaps the SDK's CLI; it does not stop the Host or cancel an accepted request. An optional `Client(timeout=...)` bounds synchronous CLI operations, separately from a dialog's `timeout_seconds`.

If a response is lost after submission, reconcile a known request ID through status or wait rather than blindly submitting again. Notifications and accepted dialogs remain owned by the Host after the submitting process exits.

## Manage task progress and final summaries

- Opt in with `updatable: true` at creation; its default is false. One-shot notifications cannot later be made updatable.
- Reuse the receipt's `request_id`. Each update supplies the latest `expected_revision` and a complete `content` replacement; success increments revision. On a conflict, fetch the current snapshot and reconcile before retrying rather than inventing a revision or overwriting another writer's work.
- Report real progress and concise facts at the cadence the workflow calls for. Use a progress block only when a 0-100 percentage is known. Keep success, failure, cancellation, and other business outcomes in the caller's content, not in inferred window states.
- Every accepted notification update restarts the display timer. Omit `duration_ms` to retain the configured duration or provide it to override that duration. Occurrence time also refreshes on accepted updates, including when hidden. Failed updates do neither. Queued/preparing notifications receive their full duration when actually presented; dialog updates preserve form values and the original interaction deadline, and reject notification-only options.
- User close hides the card and silences ordinary updates; automatic timeout permits the next report to show again unless the user previously closed it. Neither case stops the underlying task.
- A selected update may explicitly request `reshow: true`. After user close, the default permits one additional presentation beyond the initial one, consumed only when actually shown. An exhausted budget returns `reshow_suppressed: true` while still accepting content. Automatic timeout/reappearance does not consume this user-close override budget.
- Send the real final result with `updatable: false`, optionally choosing `reshow: true` and a final `duration_ms` when another presentation is warranted. Sealing is independent of visibility: it saves the final content and rejects future updates, without forcing a hidden result to appear. There is no `final` field.
- `dismiss` is caller cancellation and ends the request outright. It is not equivalent to the user hiding a card. Do not recreate an ID to bypass a close, a sealed/expired channel, or a consumed reshow budget.
- After 12 hours without an accepted report, the channel becomes `stale` and rejects updates. Host restart restores live records hidden while preserving revision, user-close intent, reshow budget, and the idle deadline. Clearing hidden history can also remove an update channel; reconcile missing/expired requests with the caller rather than silently resurrecting them.

Use `coalesce_key` for one live business slot when duplicate creation is possible. A duplicate create returns the existing request and current revision without replacing its content; a terminal request releases the key.

## Design content and actions

Write for the person deciding now, not for a debug log.

- **Title:** state the result, action, or question in one short line.
- **Paragraphs and lists:** summarize consequences, next steps, or peer facts. For many affected items, show a count and representative scope; leave the full report in the caller's normal report location.
- **Properties:** use short `label` / `text` items for decision-relevant facts such as counts, destination, size, duration, or an error reason. Either side may be omitted, but each item needs visible content.
- **Forms:** collect only the values a dialog needs. Use `radio_group` for a small exclusive choice, `select` for a longer choice list, `text` or `textarea` for short text, and bounded `number` fields for numeric input. Omit unnecessary labels instead of emitting empty ones. Keep field and action IDs as stable ASCII identifiers even when their labels are localized.
- **Actions:** prefer a clear proceed action and a safe cancel action. Use `danger` for destructive consequences, `primary` for ordinary confirmation, and `secondary` for cancel/back. Use a required checkbox when explicit acknowledgement of a consequence is needed.
- A submit action uses `submit: true` to validate and return form values. `result_status: "confirmed"` is an affirmative exit without form submission; `"cancelled"` and `"closed"` express negative exits. Add a custom `exit_code` only when the caller needs it, and still parse the result.
- Notification requests cannot contain forms or actions. Use a dialog when a reply is required.
- Keep secrets, full personal data, stack traces, item-by-item dumps, and implementation narration out of windows and form responses.

## Images, charts, and reading

Use an image or a compact chart when it materially improves the summary. Both dialogs and notifications accept image/chart blocks and an optional `content.icon`.

- Use `image_source()` for local SVG/PNG/JPEG/WebP or `svg_source()` for SVG text, then `image_block()` to embed it. Sources are self-contained data URIs with meaningful `alt` text, so history does not depend on a file continuing to exist. SVG scripts and external resources are rejected.
- Use `chart_block()` with an inline Vega-Lite 6 specification. Encoded-field tooltips are enabled by default; `encoding.tooltip` customizes or disables them. Declare Vega-Lite `params` for interactions such as hover selection, highlighting, or scale-bound zoom/pan. Do not use remote data URLs, links, or injected scripts.
- Visual height is 64-1200, an image data URI is limited to 256 KiB, a chart spec to 128 KiB, and complete content to 768 KiB. Prefer a concise visual rather than increasing the window to fit a full report.
- Set notification-only `requires_read: true` when an unread historical result should continue requiring attention after the card disappears. It defaults to false. Tray reminders depend on unread hidden/archived entries requiring reading, not visible or queued cards. Reading a detail acknowledges its content revision; it does not execute an action or approve a business operation. Dialog form values are not persisted in notification history.

## Layout and templates

Use built-in `notification` for notifications and `confirm` for confirmation or a small form; `form` is also available. Notifications use the primary display work area and are always on top without requesting focus. Do not send notification `always_on_top: false` or `area: "screen"`; dialogs may configure topmost, display, and area independently.

Leave width and height unset unless the content needs a deliberate size. Dimensions are logical pixels or percentage strings, not manually DPI-scaled physical pixels. Order content as conclusion, key facts, implications, progress, and required input. Use `content.vertical_align: "start"` for dense content; ordinary short content defaults to `center`. Notification scheduling is FIFO with at most three visible cards; it does not wait behind the dialog queue.

Choose an external template only as a caller-selected, trusted local directory through CLI `--template` or SDK `template=Path(...)`. Request JSON cannot select a template path or inject HTML/CSS/JavaScript. Validate a custom template with `popu templates validate --path <directory>` before relying on it.
