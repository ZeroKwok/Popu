import { readFileSync, readdirSync } from "node:fs";
import { test, expect } from "@playwright/test";

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const examples = readdirSync("protocol/examples").filter((name) => /^(dialog|notification)-visual-.*\.json$/.test(name));

async function mockHost(page, fixture, history = [], foreground = true) {
  fixture = { ...fixture, actions: (fixture.actions ?? []).map((action) => ({ style: "primary", submit: false, ...action })) };
  const mode = fixture.mode === "notify" ? "notification" : "confirm";
  const template = {
    html: readFileSync(`ui/templates/${mode}/template.html`, "utf8"),
    css: readFileSync(`ui/templates/${mode}/template.css`, "utf8"),
  };
  await page.addInitScript(({ fixture, template, history, foreground }) => {
    window.__reads = [];
    window.__submitted = null;
    window.__presented = false;
    window.__failed = null;
    window.__historyForeground = foreground;
    window.__detailRequests = [];
    window.__TAURI__ = {
      core: { invoke: async (command, args) => {
        if (command === "get_bootstrap") return { request: fixture, template, maximized: false };
        if (command === "ui_presented") { window.__presented = true; return; }
        if (command === "render_failed") { window.__failed = args.message; return; }
        if (command === "history_list") return history.map((item) => ({ ...structuredClone(item), content: { title: item.content.title, severity: item.content.severity } }));
        if (command === "history_get") { window.__detailRequests.push(args.requestId); return structuredClone(history.find((item) => item.request_id === args.requestId)); }
        if (command === "history_mark_read") { if (!window.__historyForeground) return null; window.__reads.push(args.requestId); return Date.now(); }
        if (command === "submit") { window.__submitted = args; return; }
        if (command === "history_clear") return 0;
        if (command === "finish") return;
        throw new Error(`Unexpected command: ${command}`);
      } },
      window: { getCurrentWindow: () => ({ startDragging: async () => {} }) },
      webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {}, onFocusChanged: async (callback) => { window.__focusCallback = callback; return () => {}; } }) },
    };
  }, { fixture, template, history, foreground });
}

for (const name of examples) {
  test(`${name} renders offline without overflow or CSP violations`, async ({ page }, testInfo) => {
    const fixture = readJson(`protocol/examples/${name}`);
    await page.setViewportSize({ width: fixture.window.width, height: fixture.window.height });
    const violations = [];
    const network = [];
    page.on("console", (message) => { if (/Content Security Policy|Refused to|EvalError/.test(message.text())) violations.push(message.text()); });
    page.on("request", (request) => { if (!request.url().startsWith("http://127.0.0.1:") && !request.url().startsWith("data:")) network.push(request.url()); });
    await mockHost(page, fixture);
    await page.goto("/index.html");
    await expect.poll(() => page.evaluate(() => window.__presented || window.__failed)).toBe(true);
    expect(await page.evaluate(() => window.__failed)).toBeNull();
    const images = page.locator(".popu-visual img");
    await expect(images).toHaveCount(1);
    expect(await images.evaluateAll((images) => images.every((image) => image.complete && image.naturalWidth > 0))).toBe(true);
    expect(await page.locator(".popu-body").evaluate((body) => body.scrollWidth <= body.clientWidth)).toBe(true);
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
  await expect(page.locator("#history-detail .popu-visual img")).toHaveJSProperty("complete", true);
  await expect(page.locator(".history-item[data-unread]")).toHaveCount(1);
  await expect(page.locator(".history-severity-icon")).toHaveCount(2);
  await page.screenshot({ path: testInfo.outputPath("history.png") });
});

test("updating a chart preserves entered form values", async ({ page }) => {
  const fixture = readJson("protocol/examples/dialog-visual-bars.json");
  await mockHost(page, fixture);
  await page.goto("/index.html");
  await expect.poll(() => page.evaluate(() => window.__presented)).toBe(true);
  await page.getByLabel("I have reviewed these measurements").check();
  const previous = await page.locator(".popu-visual img").getAttribute("src");
  const content = structuredClone(fixture.content);
  content.blocks[1].spec.data.values[0].ms = 200;
  await page.evaluate((content) => window.popuApplyUpdate(content), content);
  await expect(page.getByLabel("I have reviewed these measurements")).toBeChecked();
  await expect.poll(() => page.locator(".popu-visual img").getAttribute("src")).not.toBe(previous);
  await expect.poll(() => page.locator(".popu-visual img").evaluate((image) => image.complete && image.naturalWidth > 0)).toBe(true);
  expect(await page.evaluate(() => window.__failed)).toBeNull();
});

test("a semantically invalid chart reports a render failure instead of starting the timer", async ({ page }) => {
  const fixture = readJson("protocol/examples/notification-visual-line.json");
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
  await expect.poll(() => page.locator(".popu-visual img").evaluate((image) => image.complete && image.naturalWidth > 0)).toBe(true);
  expect(await page.evaluate(() => window.__reads)).toEqual([]);
  await page.evaluate(() => { window.__focusCallback({ payload: false }); window.__historyForeground = true; window.__focusCallback({ payload: true }); });
  await expect.poll(() => page.evaluate(() => window.__reads)).toEqual(["0"]);
  expect(await page.evaluate(() => window.__detailRequests)).toEqual(["0"]);
});
