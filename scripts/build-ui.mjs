import { build } from "esbuild";
import { readFile, readdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const result = await build({
  entryPoints: ["scripts/chart-worker.js", "scripts/chart-runtime.js"],
  outdir: "ui/vendor",
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
  const directory = resolve(path);
  const metadata = JSON.parse(await readFile(`${directory}/package.json`, "utf8"));
  const names = (await readdir(directory)).filter((name) => /^licen[cs]e(?:\..*)?$/i.test(name));
  const texts = await Promise.all(names.map((name) => readFile(`${directory}/${name}`, "utf8")));
  return `${metadata.name} ${metadata.version} (${metadata.license ?? "see license"})\n${texts.join("\n")}\n`;
}));
await writeFile("ui/vendor/THIRD_PARTY_LICENSES.txt", licenses.join("\n----------------------------------------\n\n"));
