import { readFileSync, readdirSync } from "node:fs";
import { test, expect } from "@playwright/test";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../..", import.meta.url));
const readJson = (path) => JSON.parse(readFileSync(resolve(root, path), "utf8"));
const fixtureNames = readdirSync(resolve(root, "protocol/examples")).filter((name) => name.endsWith(".json"));
const examples = fixtureNames.filter((name) => /^(dialog|notification)-visual-.*\.json$/.test(name));
// Long-content examples exercise scrolling on purpose; the custom template
// does not use the built-in body. All remaining summaries should fit at their
// configured window size, including the title, timestamp and action areas.
const scrollingExamples = new Set(["dialog-all-elements.json", "dialog-long-content.json", "fullscreen-ultra-long-content.json"]);
const fittingExamples = fixtureNames.filter((name) => !scrollingExamples.has(name) && name !== "notification-custom-template.json");

function fixtureViewport(fixture) {
  const dimension = (value, fallback, limit) => value === undefined ? fallback
    : typeof value === "string" ? Math.round(parseFloat(value) * limit / 100) : value;
  return {
    width: dimension(fixture.window?.width, fixture.mode === "notify" ? 360 : 440, 1920),
    height: dimension(fixture.window?.height, fixture.mode === "notify" ? 220 : 300, 1040),
  };
}

async function mockHost(page, fixture, history = [], foreground = true, opened = true) {
  await page.exposeFunction("resizeNative", (size) => page.setViewportSize(size));
  fixture = { ...fixture, actions: (fixture.actions ?? []).map((action) => ({ style: "primary", submit: false, ...action })) };
  const mode = fixture.mode === "notify" ? "notification" : "confirm";
  const template = {
    html: readFileSync(resolve(root, `ui/dist/templates/${mode}/template.html`), "utf8"),
    css: readFileSync(resolve(root, `ui/dist/templates/${mode}/template.css`), "utf8"),
  };
  await page.addInitScript(({ fixture, template, history, foreground, opened }) => {
    window.__reads = [];
    window.__submitted = null;
    window.__submissions = [];
    window.__finished = [];
    window.__presented = false;
    window.__readyRevisions = [];
    window.__failed = null;
    window.__historyForeground = foreground;
    window.__detailRequests = [];
    window.__dragStarts = 0;
    window.__historyPresented = false;
    window.__readAttempts = 0;
    window.__listRequests = 0;
    window.__historyListeners = {};
    window.__historyRecords = history;
    window.__TAURI__ = {
      core: { invoke: async (command, args) => {
        if (["submit", "finish"].includes(command) && window.__rejectNextAction) {
          window.__rejectNextAction = false;
          throw '{"code":"revision_conflict","current_revision":2}';
        }
        if (command === "get_bootstrap") return { request: fixture, revision: 1, occurred_at_ms: Date.now() - 15000, template, maximized: fixture.window?.width === "100%" && fixture.window?.height === "100%" };
        if (command === "ui_ready") { window.popuLockSize?.(); window.__readyRevisions.push(args.revision); window.__presented = true; return; }
        if (command === "fit_window") {
          const min = fixture.mode === "dialog" ? 160 : 120;
          const max = fixture.mode === "dialog" ? 720 : 480;
          await window.resizeNative({ width: innerWidth, height: Math.max(min, Math.min(max, args.measurement?.height ?? (fixture.mode === "dialog" ? 300 : 220))) });
          return true;
        }
        if (command === "render_failed") { window.__failed = args.message; return; }
        if (command === "history_presented") { window.__historyPresented = true; return opened; }
        if (command === "history_trace") return;
        if (command === "history_list") { window.__listRequests++; return history.map((item) => ({ ...structuredClone(item), content: { title: item.content.title, severity: item.content.severity } })); }
        if (command === "history_get") { window.__detailRequests.push(args.requestId); return structuredClone(history.find((item) => item.request_id === args.requestId)); }
        if (command === "history_mark_read") { window.__readAttempts++; if (!window.__historyForeground) return null; window.__reads.push(args.requestId); return Date.now(); }
        if (command === "submit") { window.__submitted = args; window.__submissions.push(args); return; }
        if (command === "history_clear") return 0;
        if (command === "finish") { window.__finished.push(args); return; }
        throw new Error(`Unexpected command: ${command}`);
      } },
      window: { getCurrentWindow: () => ({ startDragging: async () => { window.__dragStarts++; } }) },
      webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async (name, callback) => { window.__historyListeners[name] = callback; return () => {}; }, onFocusChanged: async (callback) => { window.__focusCallback = callback; return () => {}; } }) },
    };
  }, { fixture, template, history, foreground, opened });
}

function formFixture(blocks, actions = [{ id: "ok", label: "OK", submit: true }]) {
  return { version: "1.0", mode: "dialog", template: "form", content: { title: "Enter details", blocks }, actions };
}

test("Enter in a single text field submits once without navigation", async ({ page }) => {
  await mockHost(page, formFixture([{ type: "form", fields: [{ id: "name", type: "text", label: "Name" }] }]));
  await page.goto("/index.html");
  await expect(page.getByRole("button", { name: "OK", exact: true })).toBeEnabled();
  await page.getByLabel("Name", { exact: true }).fill("Alice");
  await page.getByLabel("Name", { exact: true }).press("Enter");
  await expect.poll(() => page.evaluate(() => window.__submissions)).toEqual([
    { revision: 1, action: "ok", values: { name: "Alice" } },
  ]);
  expect(new URL(page.url()).search).toBe("");
  await expect(page.getByLabel("Name", { exact: true })).toHaveValue("Alice");
});

test("Enter validates all form blocks and collects their values", async ({ page }) => {
  await mockHost(page, formFixture([
    { type: "form", fields: [{ id: "name", type: "text", label: "Name" }, { id: "team", type: "text", label: "Team" }] },
    { type: "form", fields: [{ id: "agreed", type: "checkbox", label: "Agree", required: true }] },
  ]));
  await page.goto("/index.html");
  await expect(page.getByRole("button", { name: "OK", exact: true })).toBeEnabled();
  await page.getByLabel("Name", { exact: true }).fill("Alice");
  await page.getByLabel("Team", { exact: true }).fill("Ops");
  await page.getByLabel("Name", { exact: true }).press("Enter");
  expect(await page.evaluate(() => window.__submissions)).toEqual([]);
  await page.getByLabel("Agree", { exact: true }).check();
  await page.getByLabel("Team", { exact: true }).press("Enter");
  await expect.poll(() => page.evaluate(() => window.__submitted?.values)).toEqual({ name: "Alice", team: "Ops", agreed: true });
  expect(new URL(page.url()).search).toBe("");
});

test("textarea Enter keeps a newline and ambiguous submit actions require a click", async ({ page }) => {
  await mockHost(page, formFixture([{ type: "form", fields: [
    { id: "name", type: "text", label: "Name" }, { id: "notes", type: "textarea", label: "Notes" },
  ] }], [{ id: "save", label: "Save", submit: true }, { id: "send", label: "Send", submit: true }]));
  await page.goto("/index.html");
  await expect(page.getByRole("button", { name: "Save", exact: true })).toBeEnabled();
  await page.getByLabel("Notes", { exact: true }).fill("First");
  await page.getByLabel("Notes", { exact: true }).press("Enter");
  await expect(page.getByLabel("Notes", { exact: true })).toHaveValue("First\n");
  await page.getByLabel("Name", { exact: true }).fill("Alice");
  await page.getByLabel("Name", { exact: true }).press("Enter");
  expect(await page.evaluate(() => window.__submissions)).toEqual([]);
  expect(new URL(page.url()).search).toBe("");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__submitted?.action)).toBe("send");
});

test("notification close captures the painted revision and Escape does not close", async ({ page }) => {
  const fixture = readJson("protocol/examples/notification-success.json");
  await mockHost(page, fixture);
  await page.goto("/index.html");
  await expect.poll(() => page.evaluate(() => window.__readyRevisions)).toEqual([1]);
  await page.keyboard.press("Escape");
  expect(await page.evaluate(() => window.__finished)).toEqual([]);
  await page.evaluate((content) => {
    window.popuApplyUpdate({ ...content, title: "Unpainted update" }, 2);
    document.querySelector("[data-popu-action='close']").click();
  }, fixture.content);
  await expect.poll(() => page.evaluate(() => window.__finished)).toEqual([
    { revision: 1, status: "closed", action: null },
  ]);
});

test("dialog actions wait for replacement content and preserve entered values", async ({ page }) => {
  const fixture = formFixture([{ type: "form", fields: [{ id: "name", type: "text", label: "Name" }] }]);
  await mockHost(page, fixture);
  await page.goto("/index.html");
  await expect(page.getByRole("button", { name: "OK", exact: true })).toBeEnabled();
  await page.getByLabel("Name", { exact: true }).fill("Alice");
  await page.evaluate((content) => {
    window.popuApplyUpdate({ ...content, title: "Review updated details" }, 2);
    document.querySelector("[data-testid='action-ok']").click();
    document.querySelector("form").requestSubmit();
    window.__duringUpdateSubmissions = window.__submissions.length;
  }, fixture.content);
  expect(await page.evaluate(() => window.__duringUpdateSubmissions)).toBe(0);
  await expect(page.getByRole("button", { name: "OK", exact: true })).toBeEnabled();
  await expect(page.getByLabel("Name", { exact: true })).toHaveValue("Alice");
  await page.getByRole("button", { name: "OK", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__submitted)).toEqual({ revision: 2, action: "ok", values: { name: "Alice" } });
});

test("a rejected stale action shows an error and can be retried without losing values", async ({ page }) => {
  await mockHost(page, formFixture([{ type: "form", fields: [{ id: "name", type: "text", label: "Name" }] }]));
  await page.goto("/index.html");
  await expect(page.getByRole("button", { name: "OK", exact: true })).toBeEnabled();
  await page.getByLabel("Name", { exact: true }).fill("Alice");
  await page.evaluate(() => { window.__rejectNextAction = true; });
  await page.getByRole("button", { name: "OK", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveText("The content changed. Please review it and try again.");
  await expect(page.getByLabel("Name", { exact: true })).toHaveValue("Alice");
  expect(await page.evaluate(() => window.__submissions)).toEqual([]);
  await page.getByRole("button", { name: "OK", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__submitted?.values)).toEqual({ name: "Alice" });
});

test("a dialog without fields reports rejected actions and Escape remains cancellation", async ({ page }) => {
  await mockHost(page, formFixture([], [{ id: "confirm", label: "Confirm", result_status: "confirmed" }]));
  await page.goto("/index.html");
  await expect(page.getByRole("button", { name: "Confirm", exact: true })).toBeEnabled();
  await page.evaluate(() => { window.__rejectNextAction = true; });
  await page.getByRole("button", { name: "Confirm", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveText("The content changed. Please review it and try again.");
  await page.keyboard.press("Escape");
  await expect.poll(() => page.evaluate(() => window.__finished)).toEqual([
    { revision: 1, status: "cancelled", action: null },
  ]);
});

for (const name of fittingExamples) {
  test(`${name} fits its configured viewport without scrolling`, async ({ page }) => {
    const fixture = readJson(`protocol/examples/${name}`);
    await page.setViewportSize(fixtureViewport(fixture));
    await mockHost(page, fixture);
    await page.goto("/index.html");
    await expect.poll(() => page.evaluate(() => window.__presented || window.__failed)).toBe(true);
    expect(await page.evaluate(() => window.__failed)).toBeNull();
    const body = page.locator(".popu-body");
    await expect(body).toBeVisible();
    expect(await body.evaluate((body) => ({
      vertical: body.scrollHeight - body.clientHeight,
      horizontal: body.scrollWidth - body.clientWidth,
    }))).toEqual({ vertical: 0, horizontal: 0 });
    if (fixture.mode === "notify") await expect(page.locator(".popu-timestamp")).not.toBeEmpty();
  });
}

test("long content remains scrollable and its end is reachable", async ({ page }) => {
  const fixture = readJson("protocol/examples/fullscreen-ultra-long-content.json");
  await page.setViewportSize({ width: 1280, height: 720 });
  await mockHost(page, fixture);
  await page.goto("/index.html");
  await expect.poll(() => page.evaluate(() => window.__presented || window.__failed)).toBe(true);
  const body = page.locator(".popu-body");
  expect(await body.evaluate((body) => body.scrollHeight > body.clientHeight)).toBe(true);
  // Real wheel input catches overflow:hidden; setting scrollTop directly can
  // still move a hidden-overflow container that the user cannot scroll.
  await body.hover();
  await page.mouse.wheel(0, 100000);
  await expect.poll(() => body.evaluate((body) =>
    Math.abs(body.scrollHeight - body.clientHeight - body.scrollTop) <= 1,
  )).toBe(true);
});

for (const name of examples) {
  test(`${name} renders offline without overflow or CSP violations`, async ({ page }, testInfo) => {
    const fixture = readJson(`protocol/examples/${name}`);
    await page.setViewportSize(fixtureViewport(fixture));
    const violations = [];
    const network = [];
    page.on("console", (message) => { if (/Content Security Policy|Refused to|EvalError/.test(message.text())) violations.push(message.text()); });
    page.on("request", (request) => { if (!request.url().startsWith("http://127.0.0.1:") && !request.url().startsWith("data:")) network.push(request.url()); });
    await mockHost(page, fixture);
    await page.goto("/index.html");
    await expect.poll(() => page.evaluate(() => window.__presented || window.__failed)).toBe(true);
    expect(await page.evaluate(() => window.__failed)).toBeNull();
    if (fixture.content.blocks.some((block) => block.type === "chart")) {
      await expect(page.locator('.popu-chart[data-ready="true"] svg')).toHaveCount(fixture.content.blocks.filter((block) => block.type === "chart").length);
    } else {
      const images = page.locator(".popu-visual img");
      await expect(images).toHaveCount(1);
      expect(await images.evaluateAll((images) => images.every((image) => image.complete && image.naturalWidth > 0))).toBe(true);
    }
    expect(await page.locator(".popu-body").evaluate((body) => body.scrollWidth <= body.clientWidth)).toBe(true);
    expect(await page.locator(".popu-body").evaluate((body) => body.scrollHeight <= body.clientHeight)).toBe(true);
    expect(violations).toEqual([]);
    expect(network).toEqual([]);
    await page.screenshot({ path: testInfo.outputPath("render.png") });
    if (name === "dialog-visual-bars.json") {
      await page.getByLabel("I have reviewed these measurements").check();
      await page.getByRole("button", { name: "Confirm review" }).click();
      await expect.poll(() => page.evaluate(() => window.__submitted?.values.reviewed)).toBe(true);
    }
  });
}

test("history waits for chart rendering before marking only the selected item read", async ({ page }, testInfo) => {
  const fixture = readJson("protocol/examples/notification-visual-line.json");
  const ordinary = readJson("protocol/examples/notification-visual-image.json");
  const items = [fixture, ordinary].map((request, index) => ({
    request_id: String(index), state: "expired", revision: 1, content: request.content,
    ended_at_ms: 1000, read_at_ms: null, requires_read: index === 0,
  }));
  await mockHost(page, fixture, items);
  await page.goto("/history.html");
  await expect.poll(() => page.evaluate(() => window.__reads)).toEqual(["0"]);
  await expect(page.locator('#history-detail .popu-chart[data-ready="true"] svg')).toBeVisible();
  await expect(page.locator(".history-item[data-unread]")).toHaveCount(1);
  await expect(page.locator(".history-item:not([data-unread]) strong")).toHaveCSS("font-weight", "700");
  await expect(page.locator(".history-item[data-unread] strong")).toHaveCSS("font-weight", "700");
  await expect(page.locator(".history-severity-icon")).toHaveCount(2);
  await page.screenshot({ path: testInfo.outputPath("history.png") });
});

test("updating a chart preserves entered form values", async ({ page }) => {
  const fixture = readJson("protocol/examples/dialog-visual-bars.json");
  await mockHost(page, fixture);
  await page.goto("/index.html");
  await expect.poll(() => page.evaluate(() => window.__presented)).toBe(true);
  await page.getByLabel("I have reviewed these measurements").check();
  const previous = await page.locator(".popu-chart svg").evaluate((svg) => svg.outerHTML);
  const content = structuredClone(fixture.content);
  content.blocks[1].spec.data.values[0].ms = 200;
  await page.evaluate((content) => window.popuApplyUpdate(content, 2), content);
  await expect(page.getByLabel("I have reviewed these measurements")).toBeChecked();
  await expect(page.locator('.popu-chart[data-ready="true"] svg')).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__readyRevisions.at(-1))).toBe(2);
  expect(await page.locator(".popu-chart svg").evaluate((svg) => svg.outerHTML)).not.toBe(previous);
  expect(await page.evaluate(() => window.__failed)).toBeNull();
});

test("rapid content revisions acknowledge only the latest completed rendering", async ({ page }) => {
  const fixture = readJson("protocol/examples/notification-visual-line.json");
  await mockHost(page, fixture);
  await page.goto("/index.html");
  await expect.poll(() => page.evaluate(() => window.__readyRevisions)).toEqual([1]);
  await page.evaluate((content) => {
    const next = structuredClone(content);
    next.title = "Intermediate";
    window.popuApplyUpdate(next, 2);
    next.title = "Latest";
    window.popuApplyUpdate(next, 3);
  }, fixture.content);
  await expect.poll(() => page.evaluate(() => window.__readyRevisions.at(-1))).toBe(3);
  expect(await page.evaluate(() => window.__readyRevisions.slice(1).every((revision) => revision === 3))).toBe(true);
  expect(await page.evaluate(() => window.__failed)).toBeNull();
});

test("notification timestamp refreshes with content and keeps an absolute occurrence", async ({ page }, testInfo) => {
  const fixture = readJson("protocol/examples/notification-success.json");
  await page.setViewportSize({ width: 440, height: 320 });
  await mockHost(page, fixture);
  await page.goto("/index.html");
  const timestamp = page.locator("[data-popu-slot='timestamp']");
  await expect(timestamp).toHaveText(/.+/);
  const previous = await timestamp.getAttribute("datetime");
  const occurredAt = Date.now();
  await page.evaluate(({ content, occurredAt }) => window.popuApplyUpdate(content, 2, occurredAt), { content: fixture.content, occurredAt });
  await expect(timestamp).toHaveAttribute("datetime", new Date(occurredAt).toISOString());
  expect(await timestamp.getAttribute("datetime")).not.toBe(previous);
  await expect(timestamp).toHaveAttribute("title", /.+/);
  await page.screenshot({ path: testInfo.outputPath("notification-timestamp.png") });
});

test("live history updates its presentation state even without a new content revision", async ({ page }) => {
  const fixture = readJson("protocol/examples/notification-success.json");
  await mockHost(page, fixture, [{ request_id: "live", state: "shown", revision: 1, content: fixture.content, occurred_at_ms: Date.now(), read_at_ms: null, updatable: true }], false);
  await page.goto("/history.html");
  await expect(page.locator(".history-metadata")).toContainText("Displayed");
  await page.evaluate(() => {
    window.__historyRecords[0].state = "hidden";
    window.__historyListeners["history-changed"]();
  });
  await expect(page.locator(".history-metadata")).toContainText("Hidden · accepting updates");
  await expect(page.locator(".history-item")).toHaveCount(1);
});

test("a semantically invalid chart reports a render failure instead of starting the timer", async ({ page }) => {
  const fixture = readJson("protocol/examples/notification-visual-line.json");
  delete fixture.content.blocks[1].spec.layer;
  fixture.content.blocks[1].spec.mark = "not-a-mark";
  await mockHost(page, fixture);
  await page.goto("/index.html");
  await expect.poll(() => page.evaluate(() => window.__failed)).not.toBeNull();
  expect(await page.evaluate(() => window.__presented)).toBe(false);
});

test("cached background history is acknowledged on focus without reloading its graphic", async ({ page }) => {
  const fixture = readJson("protocol/examples/notification-visual-line.json");
  await mockHost(page, fixture, [{ request_id: "0", state: "expired", revision: 1, content: fixture.content, ended_at_ms: 1000, read_at_ms: null, requires_read: true }], false);
  await page.goto("/history.html");
  await expect(page.locator('.popu-chart[data-ready="true"] svg')).toBeVisible();
  expect(await page.evaluate(() => window.__reads)).toEqual([]);
  await page.evaluate(() => { window.__focusCallback({ payload: false }); window.__historyForeground = true; window.__focusCallback({ payload: true }); });
  await expect.poll(() => page.evaluate(() => window.__reads)).toEqual(["0"]);
  expect(await page.evaluate(() => window.__detailRequests)).toEqual(["0"]);
});

test("history shell appears while chart dependencies are still blocked", async ({ page }) => {
  const fixture = readJson("protocol/examples/notification-visual-line.json");
  let release;
  const blocked = new Promise((resolve) => { release = resolve; });
  await page.route("**/vendor/*.js", async (route) => { await blocked; await route.continue(); });
  await mockHost(page, fixture, [{ request_id: "0", state: "expired", revision: 1, content: fixture.content, ended_at_ms: 1000, read_at_ms: null, requires_read: true }]);
  await page.goto("/history.html");
  try {
    await expect(page.getByRole("heading", { name: "Notification history", exact: true })).toBeVisible();
    await expect(page.locator(".history-item")).toHaveCount(1);
    expect(await page.evaluate(() => window.__historyPresented)).toBe(true);
    expect(await page.evaluate(() => window.__reads)).toEqual([]);
    await expect(page.locator('.popu-chart[data-ready="true"]')).toHaveCount(0);
  } finally { release(); }
  await expect.poll(() => page.evaluate(() => window.__reads)).toEqual(["0"]);
});

test("initial native focus rejection recovers without switching history rows", async ({ page }) => {
  const fixture = readJson("protocol/examples/notification-visual-image.json");
  await mockHost(page, fixture, [{ request_id: "0", state: "expired", revision: 1, content: fixture.content, ended_at_ms: 1000, read_at_ms: null, requires_read: true }], false);
  await page.goto("/history.html");
  await expect.poll(() => page.evaluate(() => window.__readAttempts)).toBeGreaterThan(0);
  await page.evaluate(() => { window.__historyForeground = true; });
  await expect.poll(() => page.evaluate(() => window.__reads)).toEqual(["0"]);
  expect(await page.evaluate(() => window.__detailRequests)).toEqual(["0"]);
});

test("history can present its shell when a hidden WebView suspends animation frames", async ({ page }) => {
  await mockHost(page, readJson("protocol/examples/notification-visual-image.json"));
  await page.addInitScript(() => {
    window.requestAnimationFrame = () => 1;
    window.cancelAnimationFrame = () => {};
  });
  await page.goto("/history.html");
  await expect.poll(() => page.evaluate(() => window.__historyPresented)).toBe(true);
  await expect(page.getByText("No notification history", { exact: true })).toBeVisible();
});

test("hidden prewarming defers history queries and detail rendering until opened", async ({ page }) => {
  const fixture = readJson("protocol/examples/notification-visual-line.json");
  await mockHost(page, fixture, [{ request_id: "0", state: "expired", revision: 1, content: fixture.content, ended_at_ms: 1000, read_at_ms: null, requires_read: true }], true, false);
  await page.goto("/history.html");
  await expect.poll(() => page.evaluate(() => window.__historyPresented)).toBe(true);
  expect(await page.evaluate(() => [window.__listRequests, window.__detailRequests.length, window.__reads.length])).toEqual([0, 0, 0]);
  await page.evaluate(() => window.__historyListeners["history-opened"]());
  await expect.poll(() => page.evaluate(() => window.__reads)).toEqual(["0"]);
});

test("bar tooltips show formatted values, stay out of window dragging and are disposed on update", async ({ page }, testInfo) => {
  const fixture = readJson("protocol/examples/dialog-visual-bars.json");
  await mockHost(page, fixture);
  await page.goto("/index.html");
  await expect(page.locator('.popu-chart[data-ready="true"]')).toBeVisible();
  const gateway = page.locator('.popu-chart path[aria-label*="Gateway"]').first();
  await gateway.hover();
  const tip = page.getByRole("tooltip");
  await expect(tip).toBeVisible();
  await expect(tip).toContainText("Gateway");
  await expect(tip).toContainText("42.0");
  await page.screenshot({ path: testInfo.outputPath("bar-tooltip.png") });
  await gateway.click();
  expect(await page.evaluate(() => window.__dragStarts)).toBe(0);
  const content = structuredClone(fixture.content);
  content.blocks[1].spec.data.values[0].ms = 200;
  await page.evaluate((content) => window.popuApplyUpdate(content, 2), content);
  await expect(page.locator('.popu-chart[data-ready="true"]')).toBeVisible();
  await expect(page.locator(".popu-chart-tooltip")).toHaveCount(1);
  await page.locator('.popu-chart path[aria-label*="Gateway"]').first().hover();
  await expect(page.getByRole("tooltip")).toContainText("200.0");
  await page.mouse.move(0, 0);
  await expect(page.getByRole("tooltip")).toHaveCount(0);
});

test("heatmap hover shows the cell coordinates and formatted value within the window", async ({ page }, testInfo) => {
  const fixture = readJson("protocol/examples/notification-visual-heatmap.json");
  await page.setViewportSize({ width: 460, height: 420 });
  await mockHost(page, fixture);
  await page.goto("/index.html");
  await page.locator('.popu-chart path[aria-label*="Tech"][aria-label*="AM"]').first().hover();
  const tip = page.getByRole("tooltip");
  await expect(tip).toBeVisible();
  await expect(tip).toContainText("Tech");
  await expect(tip).toContainText("AM");
  await expect(tip).toContainText("+1.20");
  const bounds = await tip.boundingBox();
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.y).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(460);
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(420);
  await page.screenshot({ path: testInfo.outputPath("heatmap-tooltip.png") });
});

test("line hover finds the nearest point without requiring an exact point hit", async ({ page }, testInfo) => {
  const fixture = readJson("protocol/examples/notification-visual-line.json");
  await page.setViewportSize({ width: 440, height: 400 });
  await mockHost(page, fixture);
  await page.goto("/index.html");
  const point = page.locator('.popu-chart .mark-symbol path[aria-label*="10:30"]').first();
  const bounds = await point.boundingBox();
  await page.mouse.move(bounds.x + bounds.width / 2 + 12, bounds.y + bounds.height / 2);
  await expect(page.getByRole("tooltip")).toContainText("10:30");
  await expect(page.getByRole("tooltip")).toContainText("801.00");
  await page.screenshot({ path: testInfo.outputPath("nearest-tooltip.png") });
});

test("declarative legend selection changes mark emphasis", async ({ page }) => {
  const fixture = readJson("protocol/examples/dialog-visual-bars.json");
  const spec = fixture.content.blocks[1].spec;
  spec.params = [{ name: "series", select: { type: "point", fields: ["service"] }, bind: "legend" }];
  spec.encoding.color = { field: "service", type: "nominal" };
  spec.encoding.opacity = { condition: { param: "series", value: 1 }, value: 0.2 };
  await mockHost(page, fixture);
  await page.goto("/index.html");
  await page.locator(".popu-chart .role-legend-label text").filter({ hasText: "Gateway" }).click();
  await expect.poll(() => page.locator(".popu-chart .mark-rect.role-mark path").evaluateAll((paths) => paths.map((path) => path.getAttribute("opacity")))).toContain("0.2");
});

test("declarative scale binding supports wheel zoom", async ({ page }) => {
  const fixture = readJson("protocol/examples/dialog-visual-bars.json");
  fixture.content.blocks[1].spec = {
    data: { values: [{ x: 1, y: 3 }, { x: 2, y: 1 }, { x: 3, y: 5 }] },
    mark: "point", params: [{ name: "viewport", select: "interval", bind: "scales" }],
    encoding: { x: { field: "x", type: "quantitative" }, y: { field: "y", type: "quantitative" } },
  };
  await mockHost(page, fixture);
  await page.goto("/index.html");
  await expect(page.locator('.popu-chart[data-ready="true"]')).toBeVisible();
  const labels = page.locator(".popu-chart .role-axis-label text");
  const previous = await labels.allTextContents();
  await page.locator(".popu-chart svg").hover();
  await page.mouse.wheel(0, -300);
  await expect.poll(() => labels.allTextContents()).not.toEqual(previous);
});

test("tooltip content is text, never executable HTML", async ({ page }) => {
  const fixture = readJson("protocol/examples/dialog-visual-bars.json");
  fixture.content.blocks[1].spec.data.values = [{ service: "<img src=x onerror=alert(1)>", ms: 10 }];
  await mockHost(page, fixture);
  await page.goto("/index.html");
  await page.locator(".popu-chart .mark-rect.role-mark path").hover();
  await expect(page.getByRole("tooltip")).toContainText("<img src=x onerror=alert(1)>");
  await expect(page.locator(".popu-chart-tooltip img")).toHaveCount(0);
});

test("explicitly disabled tooltips stay disabled", async ({ page }) => {
  const fixture = readJson("protocol/examples/dialog-visual-bars.json");
  fixture.content.blocks[1].spec.encoding.tooltip = null;
  await mockHost(page, fixture);
  await page.goto("/index.html");
  await page.locator(".popu-chart .mark-rect.role-mark path").first().hover();
  await expect(page.getByRole("tooltip")).toHaveCount(0);
});
