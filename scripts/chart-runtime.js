import { parse, View } from "vega";
import { expressionInterpreter } from "vega-interpreter";

const loader = {
  async load() { throw new Error("Charts must use inline data"); },
  async sanitize() { throw new Error("Chart resource URLs are disabled"); },
};

export function createChartView(container, compiled, tooltip) {
  return new View(parse(compiled, null, { ast: true }), {
    renderer: "svg", container, expr: expressionInterpreter, loader, tooltip,
  }).hover();
}
