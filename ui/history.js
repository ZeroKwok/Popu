import { renderReadOnlyBlocks } from "./content-renderer.js";

const invoke = window.__TAURI__.core.invoke;
const list = document.getElementById("history-list");
const detail = document.getElementById("history-detail");
let items = [];
let selectedId = null;

const statusLabels = {
  expired: "Timed out",
  closed: "Closed by user",
  cancelled: "Dismissed",
  failed: "Failed",
  confirmed: "Confirmed",
};

const severityIcons = { info: "i", success: "✓", warning: "!", error: "×" };

function formatStatus(state) {
  return statusLabels[state] ?? state.replaceAll("_", " ");
}

function formatTime(timestamp) {
  return new Date(timestamp).toLocaleString();
}

function renderDetail(item) {
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
  const icon = document.createElement("div");
  const severity = content.severity ?? "info";
  icon.className = "popu-icon";
  icon.dataset.severity = severity;
  icon.textContent = severityIcons[severity] ?? "i";
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
}

function renderList() {
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
    const title = document.createElement("strong");
    title.textContent = item.content?.title ?? "Untitled notification";
    const summary = document.createElement("span");
    summary.textContent = `${formatStatus(item.state)} · ${formatTime(item.ended_at_ms)}`;
    row.append(title, summary);
    row.addEventListener("click", () => {
      selectedId = item.request_id;
      renderList();
      renderDetail(item);
    });
    list.append(row);
  }
}

async function refresh() {
  items = await invoke("history_list");
  if (!items.some((item) => item.request_id === selectedId)) selectedId = items[0]?.request_id ?? null;
  renderList();
  renderDetail(items.find((item) => item.request_id === selectedId));
}

document.getElementById("clear").addEventListener("click", async () => { await invoke("history_clear"); await refresh(); });
refresh().catch((error) => { detail.textContent = String(error); });
