import assert from "node:assert/strict";
import { setImmediate } from "node:timers/promises";
import test from "node:test";
import { createReadTracker } from "../ui/history-reading.js";

const item = (id, requiresRead = false) => ({ request_id: id, requires_read: requiresRead, read_at_ms: null });

function harness(markRead = async () => 123) {
  const paints = [];
  const calls = [];
  const acknowledged = [];
  const errors = [];
  const retries = new Map();
  let nextRetry = 0;
  let foreground = true;
  const tracker = createReadTracker({
    afterPaint: () => new Promise((resolve) => paints.push(resolve)),
    isForeground: () => foreground,
    markRead: (id) => { calls.push(id); return markRead(id); },
    onRead: (id, timestamp) => acknowledged.push([id, timestamp]),
    onError: (error) => errors.push(error),
    scheduleRetry: (callback) => { const id = ++nextRetry; retries.set(id, callback); return id; },
    cancelRetry: (id) => retries.delete(id),
  });
  return {
    tracker, calls, acknowledged, errors,
    setForeground(value) { foreground = value; },
    async paint() { paints.splice(0).forEach((resolve) => resolve()); await setImmediate(); },
    async retry() { const callbacks = [...retries.values()]; retries.clear(); callbacks.forEach((callback) => callback()); await setImmediate(); },
    get retries() { return retries.size; },
  };
}

test("only the currently rendered detail is read, including ordinary unread items", async () => {
  const h = harness();
  const first = item("required", true);
  const second = item("ordinary");
  h.tracker.present(first);
  h.tracker.present(second);
  assert.deepEqual(h.calls, []);
  await h.paint();
  assert.deepEqual(h.calls, ["ordinary"]);
  assert.equal(first.read_at_ms, null);
  assert.equal(second.read_at_ms, 123);
});

test("background or minimized presentation stays unread until foreground presentation", async () => {
  const h = harness();
  const selected = item("selected", true);
  h.setForeground(false);
  h.tracker.present(selected);
  await h.paint();
  assert.deepEqual(h.calls, []);
  h.setForeground(true);
  h.tracker.recheck();
  await h.paint();
  assert.deepEqual(h.calls, ["selected"]);
});

test("losing focus or clearing history before paint cancels acknowledgement", async () => {
  const h = harness();
  h.tracker.present(item("removed"));
  h.tracker.present(null);
  await h.paint();
  assert.deepEqual(h.calls, []);
});

test("refresh and repeated focus do not duplicate an in-flight acknowledgement", async () => {
  let resolve;
  const h = harness(() => new Promise((done) => { resolve = done; }));
  h.tracker.present(item("selected"));
  await h.paint();
  h.tracker.present(item("selected"));
  h.tracker.recheck();
  await h.paint();
  assert.deepEqual(h.calls, ["selected"]);
  resolve(456);
  await setImmediate();
  assert.deepEqual(h.acknowledged, [["selected", 456]]);
});

test("a backend focus rejection does not pretend the item was read", async () => {
  const h = harness(async () => null);
  const selected = item("selected");
  h.tracker.present(selected);
  await h.paint();
  assert.equal(selected.read_at_ms, null);
  assert.deepEqual(h.acknowledged, []);
});

test("failed writes remain unread and may be retried on the next presentation", async () => {
  let failed = true;
  const h = harness(async () => { if (failed) throw new Error("write failed"); return 789; });
  const selected = item("selected");
  h.tracker.present(selected);
  await h.paint();
  assert.equal(selected.read_at_ms, null);
  assert.equal(h.errors.length, 1);
  failed = false;
  h.tracker.recheck();
  await h.paint();
  assert.equal(selected.read_at_ms, 789);
});

test("already read details do not submit another write", async () => {
  const h = harness();
  h.tracker.present({ ...item("read"), read_at_ms: 42 });
  await h.paint();
  assert.deepEqual(h.calls, []);
});

test("native focus rejection retries the same detail without another selection or focus event", async () => {
  let eligible = false;
  const h = harness(async () => eligible ? 456 : null);
  const selected = item("selected");
  h.tracker.present(selected);
  await h.paint();
  assert.equal(h.retries, 1);
  eligible = true;
  await h.retry();
  await h.paint();
  assert.equal(selected.read_at_ms, 456);
  assert.deepEqual(h.calls, ["selected", "selected"]);
  assert.equal(h.retries, 0);
});

test("missing initial DOM focus recovers automatically", async () => {
  const h = harness();
  h.setForeground(false);
  h.tracker.present(item("selected"));
  await h.paint();
  assert.equal(h.retries, 1);
  h.setForeground(true);
  await h.retry();
  await h.paint();
  assert.deepEqual(h.calls, ["selected"]);
});

test("blur cancels retries and focus resumes the retained selection", async () => {
  const h = harness(async () => null);
  h.tracker.present(item("selected"));
  await h.paint();
  h.tracker.setActive(false);
  assert.equal(h.retries, 0);
  await h.retry();
  await h.paint();
  assert.deepEqual(h.calls, ["selected"]);
  h.tracker.setActive(true);
  await h.paint();
  assert.deepEqual(h.calls, ["selected", "selected"]);
});

test("switching selection during a rejected write retries only the new selection", async () => {
  let rejectFirst;
  const h = harness((id) => id === "first" ? new Promise((resolve) => { rejectFirst = resolve; }) : Promise.resolve(999));
  const first = item("first");
  const second = item("second");
  h.tracker.present(first);
  await h.paint();
  h.tracker.present(second);
  rejectFirst(null);
  await h.paint();
  await h.retry();
  await h.paint();
  assert.equal(first.read_at_ms, null);
  assert.equal(second.read_at_ms, 999);
  assert.deepEqual(h.calls, ["first", "second"]);
});

test("clearing or disposing cancels retries, including an in-flight rejection", async () => {
  let reject;
  const h = harness(() => new Promise((resolve) => { reject = resolve; }));
  h.tracker.present(item("selected"));
  await h.paint();
  h.tracker.dispose();
  reject(null);
  await setImmediate();
  assert.equal(h.retries, 0);
  h.tracker.recheck();
  await h.paint();
  assert.deepEqual(h.calls, ["selected"]);
});
