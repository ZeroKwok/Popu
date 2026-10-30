export function formatNotificationTime(timestamp, now = Date.now()) {
  const seconds = Math.max(0, Math.floor((now - timestamp) / 1000));
  const formatter = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  for (const [unit, size] of [["day", 86400], ["hour", 3600], ["minute", 60]]) {
    if (seconds >= size) return formatter.format(-Math.floor(seconds / size), unit);
  }
  return formatter.format(-seconds, "second");
}

export function renderNotificationTime(target, timestamp) {
  if (!target || !Number.isFinite(timestamp)) return;
  const date = new Date(timestamp);
  target.dateTime = date.toISOString();
  target.title = date.toLocaleString();
  target.textContent = formatNotificationTime(timestamp);
}
