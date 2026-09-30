// Eases the mouse wheel on a vertical scroller: each notch moves a target and
// the scroller glides to it, instead of jumping a hundred pixels at once.
// Trackpads already scroll smoothly (and with their own momentum), so anything
// that looks like one is left to the browser, as are inner scrollers that can
// still move and the system's reduced-motion setting.
import { type RefObject, useEffect } from "react";

const reducedMotion = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

const TAU = 85; // ms: how quickly the scroller catches up with its target
const NOTCH = 50; // smaller wheel deltas come from trackpads and precision wheels
const TOUCHPAD_MEMORY = 600; // ms a small delta marks the device as a trackpad

/** An ancestor between the event target and `root` that can still scroll that way. */
function innerScroller(target: EventTarget | null, root: HTMLElement, dy: number): boolean {
  for (let n = target as HTMLElement | null; n && n !== root; n = n.parentElement) {
    if (!(n instanceof HTMLElement)) continue;
    const oy = getComputedStyle(n).overflowY;
    if ((oy === "auto" || oy === "scroll") && n.scrollHeight > n.clientHeight + 1) {
      if (dy < 0 ? n.scrollTop > 0 : n.scrollTop + n.clientHeight < n.scrollHeight - 1) return true;
    }
  }
  return false;
}

export function useSmoothWheel(ref: RefObject<HTMLElement | null>, enabled = true) {
  useEffect(() => {
    const el = ref.current;
    if (!el || !enabled) return;
    let target = 0;
    let shown = 0; // the last position this code wrote
    let raf = 0;
    let running = false;
    let touchpadUntil = 0;

    const frame = (() => {
      let prev = 0;
      const step = (now: number) => {
        // Something else moved the scroller (keyboard focus, restore): give way.
        if (Math.abs(el.scrollTop - shown) > 2) {
          running = false;
          return;
        }
        const dt = Math.min(now - prev, 50);
        prev = now;
        const cur = el.scrollTop + (target - el.scrollTop) * (1 - Math.exp(-dt / TAU));
        const done = Math.abs(target - cur) < 0.5;
        el.scrollTop = done ? target : cur;
        shown = el.scrollTop;
        if (done) running = false;
        else raf = requestAnimationFrame(step);
      };
      return (now: number) => {
        prev = now;
        step(now);
      };
    })();

    const onWheel = (e: WheelEvent) => {
      if (e.ctrlKey || e.defaultPrevented || reducedMotion() || Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
      const line = e.deltaMode === WheelEvent.DOM_DELTA_LINE;
      const notch = line || (e.deltaMode === WheelEvent.DOM_DELTA_PIXEL && Math.abs(e.deltaY) >= NOTCH);
      if (!notch) touchpadUntil = e.timeStamp + TOUCHPAD_MEMORY;
      if (!notch || e.timeStamp < touchpadUntil) return;
      if (innerScroller(e.target, el, e.deltaY)) return;
      const limit = el.scrollHeight - el.clientHeight;
      if (limit <= 0) return;
      e.preventDefault();
      const px = line ? e.deltaY * 40 : e.deltaY;
      if (!running) target = el.scrollTop;
      target = Math.max(0, Math.min(limit, target + px));
      if (!running) {
        running = true;
        shown = el.scrollTop;
        raf = requestAnimationFrame(frame);
      }
    };

    el.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      cancelAnimationFrame(raf);
      el.removeEventListener("wheel", onWheel);
    };
  }, [ref, enabled]);
}
