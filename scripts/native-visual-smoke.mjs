// Windows/WebView2 smoke test. Requires no running Popu Host; history is isolated.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, mkdir, readFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

const executable = resolve("src-tauri/target/debug/popu.exe");
await mkdir("test-results", { recursive: true });
const directory = await mkdtemp(resolve("test-results/native-"));
const environment = { ...process.env, LOCALAPPDATA: directory };

function cli(args, input) {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, args, { env: environment, windowsHide: true });
    let stdout = "", stderr = "";
    const timer = setTimeout(() => { child.kill(); reject(new Error(`CLI timeout: ${args.join(" ")}`)); }, 25000);
    child.stdout.on("data", (data) => { stdout += data; });
    child.stderr.on("data", (data) => { stderr += data; });
    child.on("error", (error) => { clearTimeout(timer); reject(error); });
    child.on("exit", (code) => { clearTimeout(timer); resolve({ code, stdout, stderr }); });
    child.stdin.end(input);
  });
}

const existing = await cli(["host", "status"]);
assert.notEqual(existing.code, 0, "Exit the existing Popu Host before the native smoke test");
const host = spawn(executable, ["host", "run"], { env: environment, windowsHide: true, stdio: ["ignore", "ignore", "pipe"] });
let errors = "";
host.stderr.on("data", (data) => { errors += data; });
let started = false;
try {
  for (let attempt = 0; attempt < 40; attempt++) {
    if (host.exitCode !== null) throw new Error(`Host exited early: ${errors}`);
    if ((await cli(["host", "status"])).code === 0) { started = true; break; }
    await delay(100);
  }
  assert(started, `Host did not start: ${errors}`);
  for (const name of ["notification-visual-line.json", "notification-visual-heatmap.json", "notification-visual-image.json"]) {
    const fixture = JSON.parse(await readFile(join("protocol/examples", name), "utf8"));
    fixture.duration_ms = 1500;
    const created = await cli(["request", "-"], JSON.stringify(fixture));
    assert.equal(created.code, 0, `${name}: ${created.stderr}\n${errors}`);
    const id = JSON.parse(created.stdout).request_id;
    const terminal = await cli(["wait", id]);
    assert.equal(terminal.code, 0, terminal.stderr);
    assert.equal(JSON.parse(terminal.stdout).state, "expired", `${name}: ${terminal.stdout}\n${errors}`);
    console.log(`PASS native WebView2: ${name}`);
  }
  const dialog = JSON.parse(await readFile("protocol/examples/dialog-visual-bars.json", "utf8"));
  dialog.timeout_seconds = 2;
  const result = await cli(["request", "-"], JSON.stringify(dialog));
  assert.equal(result.code, 11, `dialog: ${result.stderr}\n${errors}`);
  assert.equal(JSON.parse(result.stdout.trim().split(/\r?\n/).at(-1)).status, "timed_out");
  console.log("PASS native WebView2: dialog-visual-bars.json");
  console.log(`Isolated history: ${directory}`);
} finally {
  if (host.exitCode === null) {
    if (started) await cli(["host", "stop"]);
    if (host.exitCode === null) await Promise.race([new Promise((resolve) => host.once("exit", resolve)), delay(2000)]);
    if (host.exitCode === null) host.kill();
  }
}
