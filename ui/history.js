import { renderReadOnlyBlocks } from "./content-renderer.js";
import { createReadTracker } from "./history-reading.js";

const invoke = window.__TAURI__.core.invoke;
const list = document.getElementById("history-list");
const detail = document.getElementById("history-detail");
let items = [];
let selectedId = null;
let refreshTimer = null;
let refreshing = false;
let refreshQueued = false;
const readTracker = createReadTracker({
  afterPaint: () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
  isForeground: () => document.visibilityState === "visible" && document.hasFocus(),
  markRead: (requestId) => invoke("history_mark_read", { requestId }),
  onRead: (requestId, timestamp) => {
    const item = items.find((item) => item.request_id === requestId);
    if (item) item.read_at_ms = timestamp;
    renderList();
  },
  onError: showError,
});

const statusLabels = {
  expired: "Timed out",
  closed: "Closed by user",
  cancelled: "Dismissed",
  failed: "Failed",
  confirmed: "Confirmed",
};

const severityIcons = { info: "i", success: "✓", warning: "!", error: "×" };

function createSeverityIcon(severity = "info") {
  const icon = document.createElement("span");
  icon.className = "popu-icon";
  icon.dataset.severity = severity;
  icon.textContent = severityIcons[severity] ?? "i";
  icon.setAttribute("role", "img");
  icon.setAttribute("aria-label", severity);
  return icon;
}

function formatStatus(state) {
  return statusLabels[state] ?? state.replaceAll("_", " ");
}

function formatTime(timestamp) {
  return new Date(timestamp).toLocaleString();
}

function renderDetail(item) {
  readTracker.present(null);
  detail.replaceChildren();
  if (!item) {
    const empty = document.createElement("p");
    empty.className = "empty";
    empty.textContent = "Select a notification to view its contents.";
    detail.append(empty);
    return;
  }

  const content = item.content ?? {};
  const heading = document.createElement("div");
  heading.className = "popu-heading";
  const icon = createSeverityIcon(content.severity);
  const title = document.createElement("h2");
  title.className = "popu-title";
  title.textContent = content.title ?? "Untitled notification";
  heading.append(icon, title);

  const metadata = document.createElement("dl");
  metadata.className = "history-metadata";
  for (const [label, value] of [
    ["Ended", formatTime(item.ended_at_ms)],
    ["Outcome", formatStatus(item.state)],
    ["Revision", String(item.revision)],
  ]) {
    const term = document.createElement("dt");
    term.textContent = label;
    const description = document.createElement("dd");
    description.textContent = value;
    metadata.append(term, description);
  }

  const body = document.createElement("div");
  body.className = "popu-content history-content";
  renderReadOnlyBlocks(body, content.blocks, content.vertical_align ?? "start");
  if (body.hidden) {
    body.hidden = false;
    const empty = document.createElement("p");
    empty.className = "history-no-content";
    empty.textContent = "This notification did not include additional content.";
    body.append(empty);
  }
  detail.append(heading, metadata, body);
  readTracker.present(item);
}

function renderList() {
  const unread = items.filter((item) => item.read_at_ms === null);
  document.getElementById("history-counts").textContent =
    `${unread.filter((item) => item.requires_read).length} awaiting reading · ${unread.length} unread`;
  list.replaceChildren();
  if (!items.length) {
    const empty = document.createElement("p");
    empty.className = "empty";
    empty.textContent = "No notification history";
    list.append(empty);
    return;
  }
  for (const item of items) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "history-item";
    row.setAttribute("role", "listitem");
    row.toggleAttribute("data-selected", item.request_id === selectedId);
    row.setAttribute("aria-current", String(item.request_id === selectedId));
    const unread = item.read_at_ms === null;
    const required = unread && item.requires_read;
    row.toggleAttribute("data-unread", unread);
    row.toggleAttribute("data-required", required);
    const severity = item.content?.severity ?? "info";
    row.setAttribute("aria-label", `${severity}, ${item.content?.title ?? "Untitled notification"}${unread ? (required ? ", awaiting reading" : ", unread") : ", read"}`);
    const icon = createSeverityIcon(severity);
    icon.classList.add("history-severity-icon");
    const text = document.createElement("span");
    text.className = "history-item-text";
    const dot = document.createElement("span");
    dot.className = "history-unread-dot";
    dot.setAttribute("aria-hidden", "true");
    const title = document.createElement("strong");
    title.textContent = item.content?.title ?? "Untitled notification";
    title.title = title.textContent;
    const summary = document.createElement("span");
    summary.className = "history-summary";
    summary.textContent = `${formatStatus(item.state)} · ${formatTime(item.ended_at_ms)}`;
    text.append(title, summary);
    if (required) {
      const badge = document.createElement("span");
      badge.className = "history-required-badge";
      badge.textContent = "Awaiting reading";
      text.append(badge);
    }
    row.append(icon, text, dot);
    row.addEventListener("click", () => {
      selectedId = item.request_id;
      renderList();
      renderDetail(item);
    });
    list.append(row);
  }
}

async function refresh() {
  if (refreshing) {
    refreshQueued = true;
    return;
  }
  refreshing = true;
  try {
    items = await invoke("history_list");
    document.getElementById("history-error").textContent = "";
    if (!items.some((item) => item.request_id === selectedId)) selectedId = items[0]?.request_id ?? null;
    renderList();
    renderDetail(items.find((item) => item.request_id === selectedId));
  } finally {
    refreshing = false;
    if (refreshQueued) {
      refreshQueued = false;
      queueMicrotask(() => refresh().catch(showError));
    }
  }
}

function showError(error) { document.getElementById("history-error").textContent = String(error); }

function scheduleRefresh() {
  if (refreshTimer !== null) return;
  refreshTimer = setTimeout(() => {
    refreshTimer = null;
    refresh().catch(showError);
  }, 50);
}

document.getElementById("clear").addEventListener("click", () => {
  invoke("history_clear").then(refresh).catch(showError);
});

async function initialize() {
  // Register before the first query so a tray action immediately after opening
  // this window cannot arrive before its listener exists.
  const historyWindow = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();
  await historyWindow.listen("history-changed", scheduleRefresh);
  await historyWindow.onFocusChanged(({ payload: focused }) => {
    if (focused) { scheduleRefresh(); readTracker.recheck(); }
    else readTracker.present(null);
  });
  window.addEventListener("focus", () => { scheduleRefresh(); readTracker.recheck(); });
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") scheduleRefresh();
  });
  await refresh();
}

initialize().catch(showError);
