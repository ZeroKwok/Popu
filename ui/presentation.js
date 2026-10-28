// Hidden WebViews may pause RAF. Bound the wait so native presentation can
// proceed; visible pages still get two frames before expensive detail work.
export function waitForPresentationFrame(timeoutMs = 100) {
  return new Promise((resolve) => {
    let frame = null;
    const finish = () => { clearTimeout(timer); cancelAnimationFrame(frame); resolve(); };
    const timer = setTimeout(finish, timeoutMs);
    frame = requestAnimationFrame(() => { frame = requestAnimationFrame(finish); });
  });
}
