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

function chartSvg(spec, width, height, signal) {
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
    const timer = setTimeout(() => finish(null, new Error("Chart rendering timed out")), 5000);
    signal.addEventListener("abort", cancel, { once: true });
    worker.onmessage = ({ data }) => finish(data.svg, data.error ? new Error(data.error) : null);
    worker.onerror = () => finish(null, new Error("Unable to load the local chart renderer"));
    worker.postMessage({ spec, width, height, dark: matchMedia("(prefers-color-scheme: dark)").matches });
  });
}

export async function renderVisualBlock(section, block, signal) {
  const figure = document.createElement("figure");
  figure.className = "popu-visual";
  const image = document.createElement("img");
  image.draggable = false;
  image.alt = block.type === "image" ? block.source.alt : (block.spec.description ?? block.title ?? "Chart");
  image.style.maxHeight = `${block.height ?? (block.type === "chart" ? 220 : 320)}px`;
  figure.append(image);
  if (block.caption) {
    const caption = document.createElement("figcaption");
    caption.textContent = block.caption;
    figure.append(caption);
  }
  section.append(figure);
  // Callers attach the section synchronously before we measure its width.
  await Promise.resolve();
  let src = block.source?.src;
  if (block.type === "chart") {
    const width = Math.max(120, Math.min(2048, Math.floor(section.getBoundingClientRect().width || 480)));
    const svg = await chartSvg(block.spec, width, block.height ?? 220, signal);
    if (svg === null) return;
    src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  }
  if (!signal.aborted) await loadImage(image, src, signal);
}
