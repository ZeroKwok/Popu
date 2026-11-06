// Measure only before the first native presentation. The Host owns limits,
// DPI conversion and the lifetime of the final size.
export function measureWindow(app = document.getElementById("app")) {
  const root = app?.querySelector("[data-popu-size-root]");
  if (!root) return null; // Unadapted external templates use the Host fallback.
  document.documentElement.classList.add("popu-measuring");
  try {
    const style = getComputedStyle(app);
    const height = root.getBoundingClientRect().height
      + parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);
    if (!Number.isFinite(height) || height <= 0) return null;
    return { width: innerWidth, height: Math.ceil(height) };
  } finally {
    document.documentElement.classList.remove("popu-measuring");
  }
}
