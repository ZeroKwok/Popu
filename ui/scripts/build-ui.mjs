import { build } from "esbuild";
import { cp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const output = resolve(root, "dist");
if (dirname(output) !== resolve(root)) throw new Error("UI output must stay inside the UI project");
await rm(output, { recursive: true, force: true });
await cp(resolve(root, "src"), output, { recursive: true });

const result = await build({
  absWorkingDir: root,
  entryPoints: ["charts/chart-worker.js", "charts/chart-runtime.js"],
  outdir: "dist/vendor",
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2022",
  minify: true,
  legalComments: "linked",
  metafile: true,
});

const packages = new Set(Object.keys(result.metafile.inputs).flatMap((path) => {
  const match = path.replaceAll("\\", "/").match(/^(.*node_modules\/(?:@[^/]+\/)?[^/]+)/);
  return match ? [match[1]] : [];
}));
const licenses = await Promise.all([...packages].sort().map(async (path) => {
  const directory = resolve(root, path);
  const metadata = JSON.parse(await readFile(`${directory}/package.json`, "utf8"));
  const names = (await readdir(directory)).filter((name) => /^licen[cs]e(?:\..*)?$/i.test(name));
  const texts = await Promise.all(names.map((name) => readFile(`${directory}/${name}`, "utf8")));
  return `${metadata.name} ${metadata.version} (${metadata.license ?? "see license"})\n${texts.join("\n")}\n`;
}));
await writeFile(resolve(output, "vendor/THIRD_PARTY_LICENSES.txt"), licenses.join("\n----------------------------------------\n\n"));
