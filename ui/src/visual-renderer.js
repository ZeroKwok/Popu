function loadImage(image, src, signal) {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) { resolve(); return; }
    const finish = (error) => {
      clearTimeout(timer);
      image.onload = image.onerror = null;
      signal?.removeEventListener("abort", cancel);
      if (error) reject(error); else resolve();
    };
    const cancel = () => finish();
    const timer = setTimeout(() => finish(new Error("Image decoding timed out")), 5000);
    signal?.addEventListener("abort", cancel, { once: true });
    image.onload = () => finish();
    image.onerror = () => finish(new Error("Unable to decode image"));
    image.src = src;
  });
}

export function renderContentIcon(element, content, signal) {
  if (!element) return Promise.resolve();
  element.replaceChildren();
  element.dataset.severity = content.severity ?? "info";
  element.toggleAttribute("data-custom-icon", Boolean(content.icon));
  if (!content.icon) {
    element.setAttribute("role", "img");
    element.setAttribute("aria-label", content.severity ?? "info");
    element.textContent = { info: "i", success: "✓", warning: "!", error: "×" }[content.severity ?? "info"];
    return Promise.resolve();
  }
  element.removeAttribute("role");
  element.removeAttribute("aria-label");
  const image = document.createElement("img");
  image.alt = content.icon.alt;
  image.draggable = false;
  element.append(image);
  return loadImage(image, content.icon.src, signal);
}

function compileChart(spec, width, height, signal) {
  return new Promise((resolve, reject) => {
    if (signal.aborted) { resolve(null); return; }
    const worker = new Worker(new URL("./vendor/chart-worker.js", import.meta.url), { type: "module" });
    const finish = (value, error) => {
      clearTimeout(timer);
      signal.removeEventListener("abort", cancel);
      worker.terminate();
      if (error) reject(error); else resolve(value);
    };
    const cancel = () => finish(null);
    const timer = setTimeout(() => finish(null, new Error("Chart compilation timed out")), 5000);
    signal.addEventListener("abort", cancel, { once: true });
    worker.onmessage = ({ data }) => finish(data.compiled, data.error ? new Error(data.error) : null);
    worker.onerror = () => finish(null, new Error("Unable to load the local chart renderer"));
    worker.postMessage({ spec, width, height, dark: matchMedia("(prefers-color-scheme: dark)").matches });
  });
}

let nextTooltipId = 0;

function chartTooltip(container) {
  const tip = document.createElement("div");
  tip.className = "popu-chart-tooltip";
  tip.id = `popu-chart-tooltip-${++nextTooltipId}`;
  tip.setAttribute("role", "tooltip");
  tip.hidden = true;
  document.body.append(tip);
  const hide = () => { tip.hidden = true; container.removeAttribute("aria-describedby"); };
  container.addEventListener("pointerleave", hide);
  document.addEventListener("scroll", hide, true);
  window.addEventListener("blur", hide);
  return {
    show(_handler, event, _item, value) {
      if (value === null || value === undefined || !container.isConnected) { hide(); return; }
      tip.replaceChildren();
      const fields = typeof value === "object" ? Object.entries(value).slice(0, 20) : [["", value]];
      if (!fields.length) { hide(); return; }
      for (const [label, field] of fields) {
        const key = document.createElement("strong");
        key.textContent = label;
        const text = document.createElement("span");
        text.textContent = String(field ?? "—").slice(0, 1000);
        tip.append(key, text);
      }
      tip.hidden = false;
      container.setAttribute("aria-describedby", tip.id);
      const bounds = tip.getBoundingClientRect();
      const pointer = event?.changedTouches?.[0] ?? event;
      let x = (pointer?.clientX ?? 0) + 12;
      let y = (pointer?.clientY ?? 0) + 12;
      if (x + bounds.width > innerWidth - 8) x -= bounds.width + 24;
      if (y + bounds.height > innerHeight - 8) y -= bounds.height + 24;
      tip.style.left = `${Math.max(8, Math.min(x, innerWidth - bounds.width - 8))}px`;
      tip.style.top = `${Math.max(8, Math.min(y, innerHeight - bounds.height - 8))}px`;
    },
    dispose() {
      hide();
      container.removeEventListener("pointerleave", hide);
      document.removeEventListener("scroll", hide, true);
      window.removeEventListener("blur", hide);
      tip.remove();
    },
  };
}

async function renderChart(container, block, signal) {
  const width = Math.max(120, Math.min(2048, Math.floor(container.getBoundingClientRect().width || 480)));
  const height = block.height ?? 220;
  const [compiled, { createChartView }] = await Promise.all([
    compileChart(block.spec, width, height, signal), import("./vendor/chart-runtime.js"),
  ]);
  if (signal.aborted || compiled === null) return;
  const tooltip = chartTooltip(container);
  let view;
  let observer;
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    observer?.disconnect();
    view?.finalize();
    tooltip.dispose();
    signal.removeEventListener("abort", dispose);
    window.removeEventListener("pagehide", dispose);
  };
  signal.addEventListener("abort", dispose, { once: true });
  window.addEventListener("pagehide", dispose, { once: true });
  try {
    view = createChartView(container, compiled, tooltip.show);
    await view.runAsync();
    if (disposed) return;
    container.dataset.ready = "true";
    let previousWidth = width;
    observer = new ResizeObserver(() => {
      const nextWidth = Math.max(120, Math.min(2048, Math.floor(container.clientWidth)));
      if (disposed || nextWidth === previousWidth) return;
      previousWidth = nextWidth;
      view.width(nextWidth).height(height).runAsync().catch((error) => {
        if (!disposed) { dispose(); container.textContent = `Unable to resize chart: ${error.message}`; }
      });
    });
    observer.observe(container);
  } catch (error) {
    dispose();
    throw error;
  }
}

export async function renderVisualBlock(section, block, signal) {
  const figure = document.createElement("figure");
  figure.className = "popu-visual";
  if (block.type === "chart") {
    const chart = document.createElement("div");
    chart.className = "popu-chart";
    chart.dataset.popuNoDrag = "";
    chart.setAttribute("aria-label", block.spec.description ?? block.title ?? "Chart");
    chart.style.minHeight = `${block.height ?? 220}px`;
    figure.append(chart);
    section.append(figure);
    await Promise.resolve();
    await renderChart(chart, block, signal);
    return;
  }
  const image = document.createElement("img");
  image.draggable = false;
  image.alt = block.source.alt;
  image.style.maxHeight = `${block.height ?? 320}px`;
  figure.append(image);
  if (block.caption) {
    const caption = document.createElement("figcaption");
    caption.textContent = block.caption;
    figure.append(caption);
  }
  section.append(figure);
  // Callers attach the section synchronously before we measure its width.
  await Promise.resolve();
  if (!signal.aborted) await loadImage(image, block.source.src, signal);
}
