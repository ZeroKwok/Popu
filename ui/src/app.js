import { renderContentBlocks } from "./content-renderer.js";
import { renderContentIcon } from "./visual-renderer.js";
import { renderNotificationTime } from "./notification-time.js";
import { measureWindow } from "./window-sizing.js";

const invoke = window.__TAURI__.core.invoke;

let bootstrap;
let renderVersion = 0;
let pendingContent = Promise.resolve();
let presentedRevision = 0;
let actionPending = false;
let sizeLocked = false;
window.popuLockSize = () => { sizeLocked = true; };

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

function renderFormBlock(block, section) {
  const form = document.createElement("form");
  form.className = "popu-form";
  form.dataset.popuForm = "";
  form.noValidate = true;
  // noValidate only skips browser validation; implicit submission can still
  // navigate/reload the WebView and lose input instead of returning a Host result.
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    submitDefaultAction();
  });
  // Browsers handle Enter differently with one versus multiple inputs. Use the
  // same Popu action in both cases, leaving IME confirmation and textarea editing alone.
  form.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" || event.isComposing || event.target.tagName === "TEXTAREA") return;
    if (!(event.target instanceof HTMLInputElement) || !["text", "number"].includes(event.target.type)) return;
    event.preventDefault();
    submitDefaultAction();
  });
  for (const field of block.fields) form.append(renderField(field));
  section.append(form);
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

function restoreValues(values) {
  for (const [id, value] of Object.entries(values)) {
    const root = document.querySelector(`[data-field-id="${id}"]`);
    if (!root) continue;
    for (const input of root.querySelectorAll("input, textarea, select")) {
      if (input.type === "checkbox") input.checked = Boolean(value);
      else if (input.type === "radio") input.checked = input.value === value;
      else input.value = value ?? "";
    }
  }
}

function renderContent(content, preserveForm = false) {
  const version = ++renderVersion;
  const values = preserveForm ? collectValues() : null;
  setText("title", content.title);
  renderContentIcon(element("icon"), content);
  pendingContent = renderContentBlocks(element("content"), content.blocks ?? [], content.vertical_align ?? "center", renderFormBlock, { waitForImages: false })
    .catch((error) => { if (version === renderVersion) throw error; });
  if (values) restoreValues(values);
  return pendingContent;
}

window.popuApplyUpdate = (content, revision, occurredAtMs) => {
  setActionsEnabled(false);
  bootstrap.request.content = content;
  bootstrap.revision = revision;
  bootstrap.occurred_at_ms = occurredAtMs ?? bootstrap.occurred_at_ms;
  renderNotificationTime(element("timestamp"), bootstrap.occurred_at_ms);
  renderContent(content, bootstrap.request.mode === "dialog");
  announceReady().catch(reportRenderFailure);
};

async function announceReady() {
  let ready;
  let revision;
  do {
    ready = pendingContent;
    revision = bootstrap.revision;
    // Images already own their final boxes and decode independently. Charts
    // retain their bounded semantic validation before readiness is announced.
    await ready;
    await waitForPresentationFrame();
    if (!sizeLocked && bootstrap.request.window?.height == null && ready === pendingContent) {
      let fitted = false;
      for (let attempt = 0; attempt < 3 && !fitted && ready === pendingContent; attempt++) {
        fitted = await invoke("fit_window", { revision, measurement: measureWindow() });
        await waitForPresentationFrame();
      }
      if (!fitted && ready === pendingContent) {
        await invoke("fit_window", { revision, measurement: null });
        await waitForPresentationFrame();
      }
    }
  } while (ready !== pendingContent);
  // Capture the painted version, not the latest Host version at click time.
  presentedRevision = revision;
  await invoke("ui_ready", { revision });
  // Another update may arrive while the Host handles readiness. Its content
  // needs its own acknowledgement before actions can become available again.
  if (ready === pendingContent) setActionsEnabled(true);
}

async function waitForPresentationFrame() {
  await Promise.race([
    (async () => {
      if (document.fonts) await document.fonts.ready;
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    })(),
    // A cloaked WebView can suspend font or frame notifications. Keep native
    // presentation bounded so the window and notification timer cannot stall.
    new Promise((resolve) => setTimeout(resolve, 250)),
  ]);
}

function showValidationError(message) {
  // Confirmation dialogs can have no fields; revision conflicts still need
  // a visible error without discarding the user's current input.
  const root = element("actions") ?? element("content");
  if (!root) return;
  let error = root.querySelector(".popu-error");
  if (!error) {
    error = document.createElement("div");
    error.className = "popu-error";
    error.setAttribute("role", "alert");
    root.append(error);
  }
  error.textContent = message;
}

function setActionsEnabled(enabled) {
  for (const button of element("actions")?.querySelectorAll("button") ?? []) {
    button.disabled = !enabled || actionPending;
  }
}

function submitDefaultAction() {
  const actions = bootstrap.request.actions.filter((action) => action.submit);
  // Multiple submit actions may have different business outcomes; Enter must
  // not silently choose one merely because it appears first.
  if (actions.length === 1) performAction(actions[0]);
}

async function performAction(action) {
  // Keyboard/form submissions bypass disabled buttons, so enforce the same
  // presentation and in-flight guards at the shared action entry point.
  if (actionPending || presentedRevision !== bootstrap.revision) return;
  const revision = presentedRevision;
  if (action.submit) {
    const forms = document.querySelectorAll("[data-popu-form]");
    if ([...forms].some((form) => !form.reportValidity())) return;
  }
  actionPending = true;
  setActionsEnabled(false);
  try {
    if (action.submit) {
      await invoke("submit", { revision, action: action.id, values: collectValues() });
    } else {
      await invoke("finish", { revision, status: action.result_status, action: action.id });
    }
  } catch (error) {
    const message = String(error);
    showValidationError(/revision_conflict|content_not_presented/.test(message)
      ? "The content changed. Please review it and try again." : message);
  } finally {
    actionPending = false;
    setActionsEnabled(presentedRevision === bootstrap.revision);
  }
}

function closeRequest(status) {
  invoke("finish", { revision: presentedRevision, status, action: null }).catch(showValidationError);
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
    button.disabled = presentedRevision !== bootstrap.revision;
    button.addEventListener("click", () => performAction(action));
    root.append(button);
  }
}

function hasScrollableOverflow(container) {
  return container.scrollHeight > container.clientHeight
    || container.scrollWidth > container.clientWidth;
}

function isScrollableContent(target) {
  const container = target.closest("[data-popu-scrollable]");
  if (!container) return false;

  // A short dialog's body still fills the card, but its visual whitespace is
  // background and should drag. Once either axis overflows, scrolling takes
  // priority everywhere in that body, including either scrollbar track.
  return hasScrollableOverflow(container);
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
  document.documentElement.classList.toggle("popu-auto-height", bootstrap.request.window?.height == null);
  app.innerHTML = bootstrap.template.html;
  const style = document.createElement("style");
  style.textContent = bootstrap.template.css;
  document.head.append(style);

  renderContent(bootstrap.request.content);
  renderNotificationTime(element("timestamp"), bootstrap.occurred_at_ms);
  if (bootstrap.request.mode === "notify") {
    const clock = setInterval(() => renderNotificationTime(element("timestamp"), bootstrap.occurred_at_ms), 1000);
    window.addEventListener("pagehide", () => clearInterval(clock), { once: true });
  }
  renderActions(bootstrap.request.actions);
  installDragRegions();

  document.querySelector("[data-popu-action='close']")?.addEventListener("click", () => closeRequest("closed"));
  document.addEventListener("keydown", (event) => {
    // Esc cancels a dialog; it must not acknowledge or hide notifications.
    if (event.key === "Escape" && bootstrap.request.mode === "dialog") {
      event.preventDefault();
      closeRequest("cancelled");
    }
  });
  await announceReady();
  document.querySelector("input, textarea, select, button")?.focus();
}

function reportRenderFailure(error) {
  document.getElementById("app").textContent = `Popu failed to render: ${error}`;
  invoke("render_failed", { message: String(error) });
}

start().catch(reportRenderFailure);
