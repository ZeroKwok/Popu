import { compile } from "vega-lite";

// Keep an explicit null tooltip effective even when the global default is on.
function preserveDisabledTooltips(spec, inheritedTooltip) {
  if (!spec || typeof spec !== "object") return spec;
  const tooltip = Object.hasOwn(spec.encoding ?? {}, "tooltip") ? spec.encoding.tooltip : inheritedTooltip;
  const result = { ...spec };
  if (spec.mark && tooltip === null) {
    result.mark = { ...(typeof spec.mark === "string" ? { type: spec.mark } : spec.mark), tooltip: false };
  }
  for (const key of ["layer", "hconcat", "vconcat", "concat"]) {
    if (Array.isArray(spec[key])) result[key] = spec[key].map((child) => preserveDisabledTooltips(child, tooltip));
  }
  if (spec.spec) result.spec = preserveDisabledTooltips(spec.spec, tooltip);
  return result;
}
self.onmessage = async ({ data: { spec, width, height, dark } }) => {
  try {
    const foreground = dark ? "#dce2ed" : "#525866";
    const grid = dark ? "#424955" : "#e5e9ef";
    const config = {
      background: "transparent",
      mark: { tooltip: { content: "encoding" } },
      view: { stroke: null },
      axis: { labelColor: foreground, titleColor: foreground, domainColor: grid, tickColor: grid, gridColor: grid },
      legend: { labelColor: foreground, titleColor: foreground },
      title: { color: foreground },
    };
    const compiled = compile({ ...preserveDisabledTooltips(spec), width, height, autosize: { type: "fit", contains: "padding" }, background: "transparent" }, { config }).spec;
    self.postMessage({ compiled });
  } catch (error) {
    self.postMessage({ error: String(error.message ?? error) });
  }
};
