import { readFileSync } from "node:fs";
import { test, expect } from "@playwright/test";

const template = (mode) => ({
  html: readFileSync(new URL(`../../dist/templates/${mode}/template.html`, import.meta.url), "utf8"),
  css: readFileSync(new URL(`../../dist/templates/${mode}/template.css`, import.meta.url), "utf8"),
});

async function open(page, { mode = "dialog", width, height, blocks = [], external = false } = {}) {
  width ??= mode === "dialog" ? 440 : 360;
  await page.setViewportSize({ width, height: height ?? 300 });
  await page.exposeFunction("resizeNative", (size) => page.setViewportSize(size));
  const assets = template(mode === "dialog" ? "confirm" : "notification");
  if (external) assets.html = assets.html.replace("data-popu-size-root", "");
  await page.addInitScript(({ mode, width, height, blocks, assets }) => {
    window.fits = [];
    window.ready = [];
    window.failure = null;
    window.submissions = [];
    window.__TAURI__ = {
      core: { invoke: async (command, args) => {
        if (command === "get_bootstrap") return {
          request: { mode, window: { width, height }, content: { title: "Sizing review", blocks },
            actions: mode === "dialog" ? [{ id: "ok", label: "OK", style: "primary", submit: true }] : [] },
          revision: 1, occurred_at_ms: Date.now(), template: assets, maximized: false,
        };
        if (command === "fit_window") {
          window.fits.push(args);
          const min = mode === "dialog" ? 160 : 120;
          const max = mode === "dialog" ? 720 : 480;
          const fallback = mode === "dialog" ? 300 : 220;
          await window.resizeNative({ width, height: Math.min(max, Math.max(min, args.measurement?.height ?? fallback)) });
          return true;
        }
        if (command === "ui_ready") {
          window.popuLockSize();
          window.ready.push(args.revision);
          return;
        }
        if (command === "render_failed") { window.failure = args.message; return; }
        if (command === "submit") { window.submissions.push(args); return; }
        if (command === "finish") return;
        throw new Error(command);
      } },
      window: { getCurrentWindow: () => ({ startDragging: async () => {} }) },
    };
  }, { mode, width, height, blocks, assets });
  await page.goto("/index.html");
  await expect.poll(() => page.evaluate(() => window.ready.length > 0 || window.failure)).toBe(true);
  expect(await page.evaluate(() => window.failure)).toBeNull();
}

const paragraph = (text) => ({ type: "paragraph", text });

for (const mode of ["dialog", "notify"]) test(`${mode} truncates a growing title and exposes its full tooltip`, async ({ page }, testInfo) => {
  await open(page, { mode });
  const initial = page.viewportSize();
  const title = "标题".repeat(39) + "终点";
  await page.evaluate((title) => window.popuApplyUpdate({ title, blocks: [] }, 2, Date.now()), title);
  await expect.poll(() => page.evaluate(() => window.ready.includes(2))).toBe(true);
  const heading = page.locator(".popu-title");
  await expect(heading).toHaveAttribute("title", title);
  await expect(heading).toHaveText(title);
  await expect(heading).toHaveCSS("text-overflow", "ellipsis");
  expect(await heading.evaluate((node) => node.scrollWidth > node.clientWidth)).toBe(true);
  if (mode === "dialog") {
    const button = page.getByRole("button", { name: "OK", exact: true });
    const buttonBounds = await button.boundingBox();
    expect(buttonBounds.y + buttonBounds.height).toBeLessThanOrEqual(initial.height);
    await button.click();
    await expect.poll(() => page.evaluate(() => window.submissions[0]?.revision)).toBe(2);
  } else {
    await expect(page.getByRole("button", { name: "Close", exact: true })).toBeInViewport();
  }
  expect(page.viewportSize()).toEqual(initial);
  expect(await page.evaluate(() => window.fits.length)).toBe(1);
  await page.screenshot({ path: testInfo.outputPath("long-title.png") });
});

for (const composition of ["facet", "vconcat"]) test(`${composition} charts scroll completely inside the reserved height`, async ({ page }, testInfo) => {
  const unit = { mark: "bar", encoding: { x: { field: "x", type: "nominal" }, y: { field: "y", type: "quantitative" } } };
  const data = { values: [{ group: "A", x: "one", y: 2 }, { group: "B", x: "one", y: 3 }] };
  const spec = composition === "facet"
    ? { data, facet: { row: { field: "group", type: "nominal" } }, spec: unit }
    : { data, vconcat: [{ ...unit, height: 220 }, { ...unit, height: 220 }] };
  await open(page, { width: 700, blocks: [{ type: "chart", height: 220, spec }] });
  const initial = page.viewportSize();
  const chart = page.locator('.popu-chart[data-ready="true"]');
  expect((await chart.boundingBox()).height).toBe(220);
  expect(await chart.evaluate((node) => node.scrollHeight > node.clientHeight)).toBe(true);
  await chart.hover();
  await page.mouse.wheel(0, 100000);
  await expect.poll(() => chart.evaluate((node) => Math.abs(node.scrollHeight - node.clientHeight - node.scrollTop) <= 1)).toBe(true);
  await chart.focus();
  await page.keyboard.press("Home");
  await expect.poll(() => chart.evaluate((node) => node.scrollTop)).toBe(0);
  expect(page.viewportSize()).toEqual(initial);
  expect(await page.evaluate(() => window.fits.length)).toBe(1);
  await page.screenshot({ path: testInfo.outputPath("scrollable-chart.png") });
});

test("short content uses the minimum and updates neither grow nor shrink the window", async ({ page }) => {
  await open(page);
  expect(page.viewportSize().height).toBe(160);
  await openNewContent(page, [paragraph("A wrapped paragraph with enough content. ".repeat(24))]);
  // Updates retain the first size and scroll instead of asking for another fit.
  expect(page.viewportSize().height).toBe(160);
  expect(await page.evaluate(() => window.fits.length)).toBe(1);
  expect(await page.locator(".popu-body").evaluate((body) => body.scrollHeight > body.clientHeight)).toBe(true);
  await openNewContent(page, [], 3);
  expect(page.viewportSize().height).toBe(160);
  expect(await page.evaluate(() => window.fits.length)).toBe(1);
});

test("the initial layout fits content beyond the minimum without a scrollbar", async ({ page }, testInfo) => {
  await open(page, { width: 500, blocks: [paragraph("Measured content that wraps at the supplied width. ".repeat(20))] });
  expect(page.viewportSize().height).toBeGreaterThan(160);
  expect(page.viewportSize().height).toBeLessThan(720);
  expect(await page.locator(".popu-body").evaluate((body) => body.scrollHeight - body.clientHeight)).toBeLessThanOrEqual(1);
  await page.screenshot({ path: testInfo.outputPath("auto-height.png") });
});

for (const mode of ["dialog", "notify"]) test(`${mode} scrolls long content without moving its header`, async ({ page }, testInfo) => {
  await open(page, { mode, blocks: [paragraph("Long content line\n".repeat(100))] });
  expect(page.viewportSize()).toEqual(mode === "dialog" ? { width: 440, height: 720 } : { width: 360, height: 480 });
  const body = page.locator(".popu-body");
  const heading = page.locator(".popu-heading");
  const titleBounds = await heading.boundingBox();
  const timestamp = page.locator(".popu-timestamp");
  const timestampBounds = mode === "notify" ? await timestamp.boundingBox() : null;
  await body.hover();
  await page.mouse.wheel(0, 100000);
  await expect.poll(() => body.evaluate((body) => Math.abs(body.scrollHeight - body.clientHeight - body.scrollTop) <= 1)).toBe(true);
  expect(await body.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
  expect(await heading.boundingBox()).toEqual(titleBounds);
  if (mode === "notify") {
    await expect(timestamp).not.toBeEmpty();
    expect(await timestamp.boundingBox()).toEqual(timestampBounds);
  } else {
    await expect(page.getByRole("button", { name: "OK", exact: true })).toBeInViewport();
  }
  await page.screenshot({ path: testInfo.outputPath("capped-height.png") });
});

test("an explicit height bypasses measurement", async ({ page }) => {
  await open(page, { height: 350, blocks: [paragraph("Line\n".repeat(100))] });
  expect(page.viewportSize().height).toBe(350);
  expect(await page.evaluate(() => window.fits)).toEqual([]);
});

test("a template without a measurable root uses the bounded fallback", async ({ page }) => {
  await open(page, { external: true });
  expect(page.viewportSize().height).toBe(300);
  expect(await page.evaluate(() => window.fits[0].measurement)).toBeNull();
});

for (const outcome of ["load", "error"]) test(`image ${outcome} preserves the preallocated box and window`, async ({ page }) => {
  // Suspend decoding itself: the source remains a protocol-valid inline image.
  await page.addInitScript(() => {
    const source = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, "src");
    Object.defineProperty(HTMLImageElement.prototype, "src", {
      get: source.get,
      set(value) { window.releaseImage = () => source.set.call(this, value); },
    });
  });
  const src = "data:image/svg+xml;base64," + Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" width="800" height="40"><rect width="800" height="40" fill="blue"/></svg>').toString("base64");
  await open(page, { blocks: [{ type: "image", source: { src, alt: "Deferred image" }, height: 180, caption: "Fixed image allocation" }] });
  const size = page.viewportSize();
  const box = page.locator(".popu-image-frame");
  expect((await box.boundingBox()).height).toBe(180);
  if (outcome === "load") {
    await page.evaluate(() => window.releaseImage());
    await expect.poll(() => page.locator(".popu-visual img").evaluate((image) => image.naturalWidth)).toBe(800);
  } else {
    await page.locator(".popu-visual img").dispatchEvent("error");
    await expect(box).toContainText("Unable to render image");
  }
  expect((await box.boundingBox()).height).toBe(180);
  expect(page.viewportSize()).toEqual(size);
  expect(await page.evaluate(() => window.fits.length)).toBe(1);
});

async function openNewContent(page, blocks, revision = 2) {
  await page.evaluate(({ blocks, revision }) => window.popuApplyUpdate({ title: "Updated content", blocks }, revision, Date.now()), { blocks, revision });
  await expect.poll(() => page.evaluate((revision) => window.ready.includes(revision), revision)).toBe(true);
}
