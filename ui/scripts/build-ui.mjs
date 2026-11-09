import { build } from "esbuild";
import { cp, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const output = resolve(root, "dist");
if (dirname(output) !== resolve(root)) throw new Error("UI output must stay inside the UI project");
const staging = await mkdtemp(resolve(root, ".dist-"));
try {
  await generate(staging);
  await syncDirectory(staging, output);
} finally {
  await rm(staging, { recursive: true, force: true });
}

async function generate(directory) {
  await cp(resolve(root, "src"), directory, { recursive: true });
  const result = await build({
    absWorkingDir: root,
    entryPoints: ["charts/chart-worker.js", "charts/chart-runtime.js"],
    outdir: resolve(directory, "vendor"),
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
    const packageDirectory = resolve(root, path);
    const metadata = JSON.parse(await readFile(`${packageDirectory}/package.json`, "utf8"));
    const names = (await readdir(packageDirectory)).filter((name) => /^licen[cs]e(?:\..*)?$/i.test(name));
    const texts = await Promise.all(names.map((name) => readFile(`${packageDirectory}/${name}`, "utf8")));
    return `${metadata.name} ${metadata.version} (${metadata.license ?? "see license"})\n${texts.join("\n")}\n`;
  }));
  await writeFile(resolve(directory, "vendor/THIRD_PARTY_LICENSES.txt"), licenses.join("\n----------------------------------------\n\n"));
}

async function syncDirectory(source, destination) {
  await mkdir(destination, { recursive: true });
  const existing = new Map((await readdir(destination, { withFileTypes: true })).map((entry) => [entry.name, entry]));

  for (const entry of await readdir(source, { withFileTypes: true })) {
    const from = resolve(source, entry.name);
    const to = resolve(destination, entry.name);
    let previous = existing.get(entry.name);
    existing.delete(entry.name);

    if (previous && !(entry.isDirectory() ? previous.isDirectory() : previous.isFile())) {
      await rm(to, { recursive: true });
      previous = undefined;
    }
    if (entry.isDirectory()) {
      await syncDirectory(from, to);
      continue;
    }

    const contents = await readFile(from);
    // Cargo tracks the timestamps of these embedded assets.
    if (!previous || !contents.equals(await readFile(to))) {
      await writeFile(to, contents);
    }
  }

  for (const name of existing.keys()) {
    await rm(resolve(destination, name), { recursive: true });
  }
}
