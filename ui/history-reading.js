// The history window acknowledges only the detail actually presented in front.
export function createReadTracker({ afterPaint, isForeground, markRead, onRead, onError }) {
  let current = null;
  let presentation = 0;
  const pending = new Set();

  async function acknowledge(item, generation) {
    if (!item || item.read_at_ms !== null || pending.has(item.request_id)) return;
    await afterPaint();
    if (generation !== presentation || !isForeground() || pending.has(item.request_id)) return;
    pending.add(item.request_id);
    try {
      const timestamp = await markRead(item.request_id);
      if (timestamp !== null) {
        item.read_at_ms = timestamp;
        onRead(item.request_id, timestamp);
      }
    } finally {
      pending.delete(item.request_id);
    }
  }

  return {
    present(item) {
      current = item;
      this.recheck();
    },
    recheck() {
      acknowledge(current, ++presentation).catch(onError);
    },
  };
}
