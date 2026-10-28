// Retain the selected, rendered detail across focus changes. Native focus
// rejection is temporary: retry without requiring another row selection.
export function createReadTracker({ afterPaint, isForeground, markRead, onRead, onError,
  scheduleRetry = (callback, delay) => setTimeout(callback, delay), cancelRetry = clearTimeout }) {
  let current = null;
  let presentation = 0;
  let active = true;
  let disposed = false;
  let retry = null;
  let retryDelay = 100;
  const pending = new Set();

  function cancel() {
    if (retry !== null) cancelRetry(retry);
    retry = null;
  }

  function retryCurrent(generation) {
    if (disposed || !active || generation !== presentation || !current || current.read_at_ms !== null || retry !== null) return;
    retry = scheduleRetry(() => { retry = null; recheck(); }, retryDelay);
    retryDelay = Math.min(retryDelay * 2, 1000);
  }

  async function acknowledge(item, generation) {
    if (!item || item.read_at_ms !== null || !active || disposed || pending.has(item.request_id)) return;
    await afterPaint();
    if (generation !== presentation || !active || disposed) return;
    if (!isForeground()) { retryCurrent(generation); return; }
    if (pending.has(item.request_id)) return;
    pending.add(item.request_id);
    let rejected = false;
    try {
      const timestamp = await markRead(item.request_id);
      rejected = timestamp === null;
      if (timestamp !== null) {
        item.read_at_ms = timestamp;
        if (current?.request_id === item.request_id) current.read_at_ms = timestamp;
        onRead(item.request_id, timestamp);
      }
    } finally {
      pending.delete(item.request_id);
      // Refresh may replace the object while a native acknowledgement is in
      // flight. Retry the current selection, never a previously selected row.
      if (rejected) retryCurrent(presentation);
    }
  }

  function recheck() {
    cancel();
    acknowledge(current, ++presentation).catch(onError);
  }

  return {
    present(item) {
      current = item;
      retryDelay = 100;
      recheck();
    },
    recheck,
    setActive(value) { active = value; retryDelay = 100; recheck(); },
    dispose() { disposed = true; current = null; ++presentation; cancel(); },
  };
}
