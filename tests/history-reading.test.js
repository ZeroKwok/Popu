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
  let foreground = true;
  const tracker = createReadTracker({
    afterPaint: () => new Promise((resolve) => paints.push(resolve)),
    isForeground: () => foreground,
    markRead: (id) => { calls.push(id); return markRead(id); },
    onRead: (id, timestamp) => acknowledged.push([id, timestamp]),
    onError: (error) => errors.push(error),
  });
  return {
    tracker, calls, acknowledged, errors,
    setForeground(value) { foreground = value; },
    async paint() { paints.splice(0).forEach((resolve) => resolve()); await setImmediate(); },
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
