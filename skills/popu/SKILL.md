---
name: popu
description: Design and invoke concise Popu human-interaction requests for automation or AI agents. Use when an automated task needs to notify a person, request a decision, collect a small amount of structured input, or safely present a destructive or ambiguous action through Popu's JSON CLI or Python SDK.
---

# Popu interaction design

Use Popu only for a bounded human interaction. It is a local, short-lived window: it does not store tasks, coordinate calls, or render untrusted HTML.

## Choose the interaction

| Need | Use |
| --- | --- |
| The task can safely continue without informing the user | Do not show Popu. |
| The user should know a completed, failed, or noteworthy outcome but need not reply | `mode: "notify"`. |
| The task needs a decision, acknowledgement, choice, or small structured value | `mode: "dialog"`. |
| The request needs a long log, complex editing, file picking, authentication, a table/tree, or a web page | Do not force it into Popu; summarize and hand off to the owning product or workflow. |

Use one dialog for one decision. Never use a dialog merely to narrate normal progress.

## Decide what to show

Write for the person deciding now, not for a debug log.

- **Title:** state the action or question in one short line: `Replace 3 existing files?`, `Backup completed`, `Choose export format`.
- **Paragraph:** use for a concise outcome, consequence, or next step.
- **Properties:** use short `label/text` items for facts that change the decision: affected count, destination, estimated size/time, conflict policy, error reason, or correlation ID. Either side may be omitted, but every item must have visible content on at least one side.
- **Unordered list:** use for peer items whose order does not change the meaning.
- **Ordered list:** use for steps, stages, or priorities.
- **Progress:** use only for a known 0-100 completion percentage.
- **Form labels:** omit `label` when a field does not need visible label text; do not emit empty label elements.
- **Form:** use one or more form blocks for the minimum structured input needed by a dialog.
- **Omit:** full paths when a meaningful leaf/name suffices, item-by-item dumps, stack traces, repeated metadata, implementation details, verbose AI reasoning, and information that cannot change the choice.
- **Sensitive data:** never put passwords, tokens, recovery keys, full personal data, or secrets in blocks, fields, or expected stdout responses.

For many affected items, show a count plus representative scope, for example `1,248 files in Documents`, not all 1,248 names. Put the full report in the calling product's normal log/report location.

## Choose controls and actions

Use the smallest control that produces an unambiguous result.

| Need | Control |
| --- | --- |
| Explicit acknowledgement before an irreversible action | Required `checkbox` with a concrete consequence. |
| One choice from 2–5 mutually exclusive options | `radio_group`. |
| One choice from a longer list | `select`. |
| A short identifier, name, or search term | `text`. |
| A short optional explanation | `textarea`; avoid it if a choice is sufficient. |
| A bounded numeric setting | `number` with `min`, `max`, and `step`. |

- Prefer two actions: a clear proceed action and a safe cancel action.
- Use `style: "danger"` only for destructive or hard-to-reverse actions; use `primary` for normal confirmation and `secondary` for cancel/back.
- A submit action must have `submit: true`; a non-submit exit action must have `result_status: "cancelled"` or `"closed"`.
- Do not create a separate button for every implementation branch. Ask for the user-level decision and let the caller map that response to execution.
- Treat `cancelled`, `closed`, and `timed_out` as expected safe outcomes. Do not continue a destructive action after any of them.

## Layout and template defaults

- Use built-in `notification` for notifications and `confirm` for a confirmation, acknowledgement, or small form. Use `form` only when it makes intent clearer.
- Use `bottom_right` for notifications and `cursor_screen_center` for dialogs unless the workflow has a stronger placement requirement.
- Leave `window.width` and `window.height` unset unless the content needs a deliberate size. Use percentages only for deliberate large/full-screen experiences, never to compensate for verbose content.
- Order blocks as the person should read them: conclusion, key facts, implications or steps, progress, then required input. Shorten and structure content before increasing the window.
- Use an external template only when it is a preinstalled, trusted local asset selected by the caller through `--template`. Do not let AI-generated request data select paths or inject HTML/CSS/JavaScript.

## Generate the request

1. Read `protocol/popu.schema.json` before emitting a new or nontrivial request. It is the authoritative field and validation contract.
2. Start from the closest file in `protocol/examples/`.
3. Set `version` to `"1.0"`; include no unknown properties.
4. Write the request JSON to a caller-controlled temporary/request file and invoke:

   ```text
   popu request <request.json>
   ```

5. For a dialog, parse the single JSON response from stdout and branch on `status`, `action`, and `values`. For a notification, treat `{ "status": "shown" }` only as confirmation that the window was created.

### Minimal completion notification

```json
{
  "version": "1.0",
  "mode": "notify",
  "template": "notification",
  "position": "bottom_right",
  "duration_ms": 6000,
  "content": {
    "severity": "success",
    "title": "Backup completed",
    "blocks": [
      { "type": "paragraph", "text": "128 files copied in 3 minutes 21 seconds." },
      { "type": "properties", "items": [{ "label": "Size", "text": "2.4 GB" }] },
      { "type": "progress", "title": "Completion", "value": 100 }
    ]
  }
}
```

### Minimal destructive confirmation

```json
{
  "version": "1.0",
  "mode": "dialog",
  "template": "confirm",
  "position": "cursor_screen_center",
  "content": {
    "severity": "warning",
    "title": "Replace 3 existing files?",
    "blocks": [
      { "type": "paragraph", "text": "The destination contains files with the same names." },
      { "type": "properties", "items": [{ "label": "Destination", "text": "D:\\Backup" }] },
      {
        "type": "form",
        "fields": [{
          "id": "confirmed",
          "type": "checkbox",
          "label": "I understand existing files will be replaced",
          "required": true
        }]
      }
    ]
  },
  "actions": [
    { "id": "replace", "label": "Replace", "style": "danger", "submit": true },
    { "id": "cancel", "label": "Cancel", "style": "secondary", "result_status": "cancelled" }
  ],
  "timeout_seconds": 300
}
```

## Verify before invoking

- Confirm the window asks for a real user decision or conveys a concise outcome.
- Confirm the title, consequence, and buttons agree.
- Confirm field IDs and action IDs are stable, ASCII identifiers meaningful to the caller.
- Confirm required fields and all terminal statuses have safe caller-side handling.
- Validate templates with `popu templates validate --path <trusted-template-dir>` when an external template is used.
