const invoke = window.__TAURI__.core.invoke;

let bootstrap;

const nonDragSelector = [
  "button",
  "input",
  "textarea",
  "select",
  "option",
  "label",
  "[data-popu-selectable]",
  "[data-popu-no-drag]",
].join(", ");

function element(slot) {
  return document.querySelector(`[data-popu-slot="${slot}"]`);
}

function setText(slot, value) {
  const target = element(slot);
  if (target) target.textContent = value ?? "";
}

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

function renderFormBlock(block, section) {
  const form = document.createElement("form");
  form.className = "popu-form";
  form.dataset.popuForm = "";
  form.noValidate = true;
  for (const field of block.fields) form.append(renderField(field));
  section.append(form);
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
  } else if (block.type === "form") {
    renderFormBlock(block, section);
  } else {
    throw new Error(`Unsupported content block: ${block.type}`);
  }
  return section;
}

function renderBlocks(blocks = [], verticalAlign = "center") {
  const root = element("content");
  if (!root) return;
  root.dataset.popuVerticalAlign = verticalAlign;
  root.replaceChildren();
  for (const block of blocks) {
    const section = renderBlock(block);
    if (section.childElementCount > 0) root.append(section);
  }
  root.hidden = root.childElementCount === 0;
}
function inputId(field) {
  return `popu-field-${field.id}`;
}

function hasValue(value) {
  // serde serializes a missing Rust Option as null. Assigning null to a DOM
  // constraint such as maxLength coerces it to 0, which blocks all typing.
  return value !== undefined && value !== null;
}

function renderField(field) {
  const wrapper = document.createElement("div");
  wrapper.className = "popu-field";
  wrapper.dataset.fieldId = field.id;

  if (field.type === "checkbox") {
    const input = document.createElement("input");
    input.id = inputId(field);
    input.name = field.id;
    input.type = "checkbox";
    input.checked = Boolean(field.default);
    input.required = Boolean(field.required);
    if (hasVisibleText(field.label)) {
      const row = document.createElement("label");
      row.className = "popu-field-inline";
      row.append(input, document.createTextNode(field.label));
      wrapper.append(row);
    } else {
      input.setAttribute("aria-label", field.id);
      wrapper.append(input);
    }
  } else if (field.type === "radio_group") {
    const options = document.createElement("div");
    options.className = "popu-options";
    for (const option of field.options ?? []) {
      const label = document.createElement("label");
      const input = document.createElement("input");
      input.type = "radio";
      input.name = field.id;
      input.value = option.value;
      input.required = Boolean(field.required);
      input.checked = field.default === option.value;
      label.append(input, document.createTextNode(option.label));
      options.append(label);
    }
    if (hasVisibleText(field.label)) {
      const legend = document.createElement("div");
      legend.className = "popu-legend";
      legend.textContent = field.label;
      wrapper.append(legend);
    } else {
      options.setAttribute("aria-label", field.id);
    }
    wrapper.append(options);
  } else {
    let input;
    if (field.type === "textarea") {
      input = document.createElement("textarea");
    } else if (field.type === "select") {
      input = document.createElement("select");
      const empty = document.createElement("option");
      empty.value = "";
      empty.textContent = field.placeholder ?? "Select…";
      input.append(empty);
      for (const option of field.options ?? []) {
        const item = document.createElement("option");
        item.value = option.value;
        item.textContent = option.label;
        input.append(item);
      }
    } else {
      input = document.createElement("input");
      input.type = field.type === "number" ? "number" : "text";
    }
    input.id = inputId(field);
    input.name = field.id;
    input.required = Boolean(field.required);
    if (hasVisibleText(field.placeholder) && field.type !== "select") {
      input.placeholder = field.placeholder;
    }
    if (hasValue(field.default)) input.value = String(field.default);
    if (hasValue(field.min_length)) input.minLength = field.min_length;
    if (hasValue(field.max_length)) input.maxLength = field.max_length;
    if (field.pattern) input.pattern = field.pattern;
    if (hasValue(field.min)) input.min = field.min;
    if (hasValue(field.max)) input.max = field.max;
    if (hasValue(field.step)) input.step = field.step;
    if (hasVisibleText(field.label)) {
      const label = document.createElement("label");
      label.htmlFor = inputId(field);
      label.textContent = field.label;
      wrapper.append(label);
    } else {
      input.setAttribute("aria-label", field.id);
    }
    wrapper.append(input);
  }

  return wrapper;
}

function requestFields() {
  return bootstrap.request.content.blocks.flatMap((block) =>
    block.type === "form" ? block.fields : [],
  );
}

function collectValues() {
  const values = {};
  for (const field of requestFields()) {
    const root = document.querySelector(`[data-field-id="${field.id}"]`);
    let value;
    if (field.type === "checkbox") {
      value = root.querySelector("input").checked;
    } else if (field.type === "radio_group") {
      value = root.querySelector("input:checked")?.value ?? null;
    } else if (field.type === "number") {
      const raw = root.querySelector("input").value;
      value = raw === "" ? null : Number(raw);
    } else if (field.type === "select") {
      const raw = root.querySelector("select").value;
      value = raw === "" ? null : raw;
    } else {
      value = root.querySelector("input, textarea, select").value;
    }
    values[field.id] = value;
  }
  return values;
}

function showValidationError(message) {
  const firstField = document.querySelector(".popu-field");
  if (!firstField) return;
  let error = firstField.querySelector(".popu-error");
  if (!error) {
    error = document.createElement("div");
    error.className = "popu-error";
    error.setAttribute("role", "alert");
    firstField.append(error);
  }
  error.textContent = message;
}

function renderActions(actions) {
  const root = element("actions");
  if (!root) return;
  root.replaceChildren();
  root.hidden = actions.length === 0;
  for (const action of actions) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `popu-button popu-button-${action.style}`;
    button.dataset.testid = `action-${action.id}`;
    button.textContent = action.label;
    button.addEventListener("click", async () => {
      try {
        if (action.submit) {
          const forms = document.querySelectorAll("[data-popu-form]");
          if ([...forms].some((form) => !form.reportValidity())) return;
          await invoke("submit", { action: action.id, values: collectValues() });
        } else {
          await invoke("finish", { status: action.result_status, action: action.id });
        }
      } catch (error) {
        showValidationError(String(error));
      }
    });
    root.append(button);
  }
}

function isScrollableContent(target) {
  const container = target.closest("[data-popu-scrollable]");
  if (!container) return false;

  // A short dialog's body still fills the card, but its visual whitespace is
  // background and should drag. Once content overflows, scrolling takes
  // priority everywhere in that body, including the scrollbar track.
  return container.scrollHeight > container.clientHeight;
}

function installDragRegions() {
  document.addEventListener("mousedown", (event) => {
    if (bootstrap.maximized) return;
    if (event.button !== 0 || !(event.target instanceof Element)) return;
    if (!event.target.closest("[data-popu-drag-region]")) return;
    if (event.target.closest(nonDragSelector)) return;
    if (isScrollableContent(event.target)) return;
    window.__TAURI__.window.getCurrentWindow().startDragging().catch((error) => {
      console.error("Popu window drag failed", error);
    });
  });
}

async function start() {
  bootstrap = await invoke("get_bootstrap");
  const app = document.getElementById("app");
  document.documentElement.classList.toggle("popu-maximized", bootstrap.maximized);
  app.innerHTML = bootstrap.template.html;
  const style = document.createElement("style");
  style.textContent = bootstrap.template.css;
  document.head.append(style);

  const severity = bootstrap.request.content.severity ?? "info";
  const icon = element("icon");
  if (icon) {
    icon.dataset.severity = severity;
    icon.textContent = { info: "i", success: "✓", warning: "!", error: "×" }[severity];
  }
  setText("title", bootstrap.request.content.title);
  renderBlocks(
    bootstrap.request.content.blocks,
    bootstrap.request.content.vertical_align ?? "center",
  );
  renderActions(bootstrap.request.actions);
  installDragRegions();

  document.querySelector("[data-popu-action='close']")?.addEventListener("click", () => invoke("finish", { status: "closed", action: null }));
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && bootstrap.request.mode === "dialog") {
      invoke("finish", { status: "cancelled", action: null });
    }
  });
  await invoke("ui_ready");
  document.querySelector("input, textarea, select, button")?.focus();
}

start().catch((error) => {
  document.getElementById("app").textContent = `Popu failed to render: ${error}`;
  invoke("render_failed", { message: String(error) });
});
