function hasVisibleText(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function appendBlockTitle(section, titleText) {
  if (!hasVisibleText(titleText)) return;
  const title = document.createElement("h2");
  title.className = "popu-block-title";
  title.textContent = titleText;
  section.append(title);
}

function renderProperties(block, section) {
  const grid = document.createElement("div");
  grid.className = "popu-properties";
  let row = 1;
  for (const item of block.items) {
    const hasLabel = hasVisibleText(item.label);
    const hasText = hasVisibleText(item.text);
    if (!hasLabel && !hasText) continue;
    if (hasLabel) {
      const label = document.createElement("div");
      label.className = "popu-property-label";
      label.style.gridRow = String(row);
      if (!hasText) label.classList.add("popu-property-full");
      label.textContent = item.label;
      grid.append(label);
    }
    if (hasText) {
      const text = document.createElement("div");
      text.className = "popu-property-text";
      text.style.gridRow = String(row);
      text.textContent = item.text;
      grid.append(text);
    }
    row += 1;
  }
  if (grid.childElementCount > 0) section.append(grid);
}

function renderList(block, section) {
  const list = document.createElement(block.type === "ordered_list" ? "ol" : "ul");
  list.className = "popu-list";
  for (const text of block.items) {
    const item = document.createElement("li");
    item.className = "popu-list-item";
    item.textContent = text;
    list.append(item);
  }
  section.append(list);
}

function renderProgress(block, section) {
  const row = document.createElement("div");
  row.className = "popu-progress-row";
  const progress = document.createElement("progress");
  progress.className = "popu-progress";
  progress.max = 100;
  progress.value = block.value;
  progress.setAttribute("aria-label", block.title ?? `Progress: ${block.value}%`);
  const value = document.createElement("span");
  value.className = "popu-progress-value";
  value.textContent = `${block.value}%`;
  row.append(progress, value);
  section.append(row);
}

function renderBlock(block) {
  const section = document.createElement("section");
  section.className = `popu-block popu-block-${block.type}`;
  appendBlockTitle(section, block.title);

  if (block.type === "paragraph") {
    const paragraph = document.createElement("p");
    paragraph.className = "popu-paragraph";
    paragraph.textContent = block.text;
    section.append(paragraph);
  } else if (block.type === "properties") {
    renderProperties(block, section);
  } else if (block.type === "unordered_list" || block.type === "ordered_list") {
    renderList(block, section);
  } else if (block.type === "progress") {
    renderProgress(block, section);
  }
  return section;
}

// History and notifications are display-only. Form blocks are deliberately
// omitted: the notify protocol rejects them and history never replays input.
export function renderReadOnlyBlocks(root, blocks = [], verticalAlign = "start") {
  if (!root) return;
  root.dataset.popuVerticalAlign = verticalAlign;
  root.replaceChildren();
  for (const block of blocks) {
    const section = renderBlock(block);
    if (section.childElementCount > 0) root.append(section);
  }
  root.hidden = root.childElementCount === 0;
}
