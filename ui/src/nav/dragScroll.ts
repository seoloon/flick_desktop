// Click and drag a horizontal scroller to move it, like flicking a tvOS shelf:
// the row follows the pointer, glides on release, and stretches with
// resistance past either end before springing back. Mouse and pen only: touch
// already scrolls natively, and remote or keyboard focus scrolls by itself. A
// press that never moves stays a click.
//
// The stretch moves the row's children with the CSS `translate` property, which
// is separate from `transform`, so it never fights a card's own scale or tilt.
import { useEffect } from "react";

const reducedMotion = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

const THRESHOLD = 6; // px the pointer must travel before a press becomes a drag
const FRICTION = 0.968; // glide decay per 1/60 s
const STRETCH = 0.5; // how far a drag can stretch past an end, as a share of the row's width
const SPRING = 190; // return spring stiffness
const DAMPING = 17; // a little under critical (~27): one soft overshoot
const BOUNCE = 0.22; // share of the glide speed that becomes stretch when it hits an end

/** Apple-style resistance: moves freely at first, then harder, never past `limit`. */
const rubber = (over: number, limit: number) => (1 - 1 / ((over * 0.55) / limit + 1)) * limit;

export function useDragScroll(el: HTMLElement | null) {
  useEffect(() => {
    if (!el) return;
    let id: number | null = null;
    let startX = 0;
    let startScroll = 0;
    let dragging = false;
    let lastX = 0;
    let lastT = 0;
    let velocity = 0; // px per ms, in scroll direction
    let raf = 0;
    let swallowClick = false;
    // Stretch past an end: px the content is pulled, and its speed.
    let pull = 0;
    let pullV = 0;
    let glideV = 0;

    const max = () => el.scrollWidth - el.clientWidth;
    const show = () => {
      const value = pull ? `${pull.toFixed(2)}px` : "";
      for (const child of Array.from(el.children) as HTMLElement[]) child.style.translate = value;
    };
    const stop = () => {
      cancelAnimationFrame(raf);
      glideV = 0;
    };

    const tick = (() => {
      let prev = 0;
      const step = (now: number) => {
        const dt = Math.min((now - prev) / 1000, 1 / 30);
        prev = now;
        if (glideV !== 0) {
          el.scrollLeft += glideV * dt * 1000;
          glideV *= FRICTION ** (dt * 60);
          const atEnd = (glideV < 0 && el.scrollLeft <= 0) || (glideV > 0 && el.scrollLeft >= max() - 0.5);
          if (atEnd) {
            // Hit the end while moving: carry some of the speed into a stretch.
            pullV = reducedMotion() ? 0 : -glideV * 1000 * BOUNCE;
            glideV = 0;
          } else if (Math.abs(glideV) < 0.006) glideV = 0;
        } else if (pull !== 0 || pullV !== 0) {
          pullV += (-SPRING * pull - DAMPING * pullV) * dt;
          pull += pullV * dt;
          if (Math.abs(pull) < 0.3 && Math.abs(pullV) < 5) pull = pullV = 0;
        }
        show();
        if (glideV !== 0 || pull !== 0 || pullV !== 0) raf = requestAnimationFrame(step);
      };
      return (now: number) => {
        prev = now;
        step(now);
      };
    })();
    const run = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(tick);
    };

    const down = (e: PointerEvent) => {
      if (e.pointerType === "touch" || e.button !== 0 || max() <= 0) return;
      id = e.pointerId;
      startX = lastX = e.clientX;
      startScroll = el.scrollLeft;
      lastT = e.timeStamp;
      velocity = 0;
      dragging = false;
      glideV = 0; // a press stops a glide on the spot
    };

    const move = (e: PointerEvent) => {
      if (e.pointerId !== id) return;
      const dx = e.clientX - startX;
      if (!dragging) {
        if (Math.abs(dx) < THRESHOLD) return;
        dragging = true;
        stop();
        pull = pullV = 0;
        el.setPointerCapture(e.pointerId);
        el.style.cursor = "grabbing";
        el.style.userSelect = "none";
      }
      const want = startScroll - dx;
      const limit = Math.max(1, el.clientWidth * STRETCH);
      if (want < 0) {
        el.scrollLeft = 0;
        pull = rubber(-want, limit);
      } else if (want > max()) {
        el.scrollLeft = max();
        pull = -rubber(want - max(), limit);
      } else {
        el.scrollLeft = want;
        pull = 0;
      }
      show();
      const dt = e.timeStamp - lastT;
      if (dt > 0) velocity = 0.75 * velocity + 0.25 * ((lastX - e.clientX) / dt);
      lastX = e.clientX;
      lastT = e.timeStamp;
    };

    const up = (e: PointerEvent) => {
      if (e.pointerId !== id) return;
      id = null;
      if (!dragging) return;
      dragging = false;
      swallowClick = true;
      // The click that follows a drag is not a press on a card.
      window.setTimeout(() => (swallowClick = false), 0);
      el.releasePointerCapture(e.pointerId);
      el.style.cursor = "";
      el.style.userSelect = "";
      // Released while stretched: spring back. A long pause before release: no glide.
      const moving = e.timeStamp - lastT < 120;
      if (pull === 0 && moving && !reducedMotion()) glideV = velocity;
      else if (reducedMotion()) pull = pullV = 0;
      pullV = 0;
      show();
      if (glideV !== 0 || pull !== 0) run();
    };

    const click = (e: MouseEvent) => {
      if (!swallowClick) return;
      e.stopPropagation();
      e.preventDefault();
    };
    // Artwork would otherwise start the browser's own image drag.
    const dragstart = (e: DragEvent) => e.preventDefault();

    el.addEventListener("pointerdown", down);
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
    el.addEventListener("click", click, true);
    el.addEventListener("dragstart", dragstart);
    const wheel = () => void (glideV = 0);
    el.addEventListener("wheel", wheel, { passive: true });
    return () => {
      stop();
      pull = pullV = 0;
      show();
      el.removeEventListener("pointerdown", down);
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      el.removeEventListener("click", click, true);
      el.removeEventListener("dragstart", dragstart);
      el.removeEventListener("wheel", wheel);
    };
  }, [el]);
}
