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

function renderDetails(details = []) {
  const root = element("details");
  if (!root) return;
  root.replaceChildren();
  root.hidden = details.length === 0;
  for (const detail of details) {
    const label = document.createElement("div");
    label.className = "popu-detail-label";
    label.textContent = detail.label;
    const value = document.createElement("div");
    value.className = "popu-detail-value";
    value.textContent = detail.value;
    root.append(label, value);
  }
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
    const row = document.createElement("label");
    row.className = "popu-field-inline";
    const input = document.createElement("input");
    input.id = inputId(field);
    input.name = field.id;
    input.type = "checkbox";
    input.checked = Boolean(field.default);
    input.required = Boolean(field.required);
    row.append(input, document.createTextNode(field.label));
    wrapper.append(row);
  } else if (field.type === "radio_group") {
    const legend = document.createElement("div");
    legend.className = "popu-legend";
    legend.textContent = field.label;
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
    wrapper.append(legend, options);
  } else {
    const label = document.createElement("label");
    label.htmlFor = inputId(field);
    label.textContent = field.label;
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
    if (field.placeholder && field.type !== "select") input.placeholder = field.placeholder;
    if (hasValue(field.default)) input.value = String(field.default);
    if (hasValue(field.min_length)) input.minLength = field.min_length;
    if (hasValue(field.max_length)) input.maxLength = field.max_length;
    if (field.pattern) input.pattern = field.pattern;
    if (hasValue(field.min)) input.min = field.min;
    if (hasValue(field.max)) input.max = field.max;
    if (hasValue(field.step)) input.step = field.step;
    wrapper.append(label, input);
  }

  const error = document.createElement("div");
  error.className = "popu-error";
  error.id = `${inputId(field)}-error`;
  error.setAttribute("aria-live", "polite");
  wrapper.append(error);
  return wrapper;
}

function renderForm(form) {
  const root = element("form");
  if (!root) return;
  root.replaceChildren();
  for (const field of form?.fields ?? []) root.append(renderField(field));
}

function collectValues() {
  const values = {};
  for (const field of bootstrap.request.form?.fields ?? []) {
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
  const first = document.querySelector(".popu-error");
  if (first) first.textContent = message;
}

function renderActions(actions) {
  const root = element("actions");
  if (!root) return;
  root.replaceChildren();
  for (const action of actions) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `popu-button popu-button-${action.style}`;
    button.dataset.testid = `action-${action.id}`;
    button.textContent = action.label;
    button.addEventListener("click", async () => {
      try {
        if (action.submit) {
          const form = element("form");
          if (form && !form.reportValidity()) return;
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

function renderProgress(progress) {
  const root = element("progress");
  if (!root || progress === null || progress === undefined) return;
  const bar = document.createElement("progress");
  bar.className = "popu-progress";
  bar.max = 100;
  bar.value = progress;
  bar.setAttribute("aria-label", `Progress: ${progress}%`);
  root.append(bar);
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
  setText("message", bootstrap.request.content.message);
  renderDetails(bootstrap.request.content.details);
  renderForm(bootstrap.request.form);
  renderActions(bootstrap.request.actions);
  renderProgress(bootstrap.request.content.progress);
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
