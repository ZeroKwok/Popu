import { compile } from "vega-lite";
import { parse, View } from "vega";
import { expressionInterpreter } from "vega-interpreter";

const loader = {
  async load() { throw new Error("Charts must use inline data"); },
  async sanitize() { throw new Error("Chart resource URLs are disabled"); },
};

self.onmessage = async ({ data: { spec, width, height, dark } }) => {
  let view;
  try {
    const foreground = dark ? "#dce2ed" : "#525866";
    const grid = dark ? "#424955" : "#e5e9ef";
    const config = {
      background: "transparent",
      view: { stroke: null },
      axis: { labelColor: foreground, titleColor: foreground, domainColor: grid, tickColor: grid, gridColor: grid },
      legend: { labelColor: foreground, titleColor: foreground },
      title: { color: foreground },
    };
    const compiled = compile({ ...spec, width, height, autosize: { type: "fit", contains: "padding" }, background: "transparent" }, { config }).spec;
    view = new View(parse(compiled, null, { ast: true }), { renderer: "none", expr: expressionInterpreter, loader });
    await view.runAsync();
    self.postMessage({ svg: await view.toSVG() });
  } catch (error) {
    self.postMessage({ error: String(error.message ?? error) });
  } finally {
    view?.finalize();
  }
};
