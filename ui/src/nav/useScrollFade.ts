// Soft edges on scrollable zones: content melts away where more of it lies
// beyond the edge, and only there (no fade at the start of a shelf that has
// not scrolled yet). The element gets `data-fade-start` / `data-fade-end`;
// the `scroll-fade-x` / `scroll-fade-y` utilities turn them into a mask.
import { type RefObject, useEffect } from "react";

export type FadeAxis = "x" | "y";

export function useScrollFade(ref: RefObject<HTMLElement | null>, axis: FadeAxis | undefined) {
  useEffect(() => {
    const el = ref.current;
    if (!el || !axis) return;
    let raf = 0;
    const update = () => {
      raf = 0;
      const pos = axis === "x" ? el.scrollLeft : el.scrollTop;
      const size = axis === "x" ? el.clientWidth : el.clientHeight;
      const total = axis === "x" ? el.scrollWidth : el.scrollHeight;
      el.toggleAttribute("data-fade-start", pos > 2);
      el.toggleAttribute("data-fade-end", pos + size < total - 2);
    };
    const schedule = () => {
      if (!raf) raf = requestAnimationFrame(update);
    };
    update();
    el.addEventListener("scroll", schedule, { passive: true });
    // Items arriving (pages loading, seasons switching) change the extent.
    const ro = new ResizeObserver(schedule);
    ro.observe(el);
    const mo = new MutationObserver(schedule);
    mo.observe(el, { childList: true, subtree: true });
    return () => {
      cancelAnimationFrame(raf);
      el.removeEventListener("scroll", schedule);
      ro.disconnect();
      mo.disconnect();
    };
  }, [ref, axis]);
}
