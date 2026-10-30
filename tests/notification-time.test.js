import assert from "node:assert/strict";
import test from "node:test";
import { formatNotificationTime, renderNotificationTime } from "../ui/notification-time.js";

test("notification time ages without changing its occurrence and exposes absolute time", () => {
  const timestamp = Date.UTC(2026, 9, 30, 6, 30);
  assert.notEqual(formatNotificationTime(timestamp, timestamp + 15000), formatNotificationTime(timestamp, timestamp + 75000));
  const target = {};
  renderNotificationTime(target, timestamp);
  assert.equal(target.dateTime, "2026-10-30T06:30:00.000Z");
  assert.equal(target.title, new Date(timestamp).toLocaleString());
  assert.ok(target.textContent);
});
