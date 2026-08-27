const invoke = window.__TAURI__.core.invoke;
const root = document.getElementById("history");

async function refresh() {
  const items = await invoke("history_list");
  root.replaceChildren();
  if (!items.length) { const empty = document.createElement("p"); empty.className = "empty"; empty.textContent = "No notification history"; root.append(empty); return; }
  for (const item of items) {
    const card = document.createElement("article");
    const title = document.createElement("h2"); title.textContent = item.content.title;
    const state = document.createElement("p"); state.textContent = `${item.state} · revision ${item.revision}`;
    const time = document.createElement("p"); time.textContent = new Date(item.ended_at_ms).toLocaleString();
    card.append(title, state, time); root.append(card);
  }
}

document.getElementById("clear").addEventListener("click", async () => { await invoke("history_clear"); await refresh(); });
refresh().catch((error) => { root.textContent = String(error); });
