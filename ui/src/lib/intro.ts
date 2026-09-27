// Plays the Flick intro over the app (App renders it): at start-up, and on
// demand while the app changes underneath (entering Flick Frame).
import { create } from "zustand";

/** `run` is bumped on each request; App shows one intro per value. While
 * `held`, the intro only covers the window (plain background) and waits. */
export const useIntro = create<{ run: number; held: boolean }>(() => ({ run: 0, held: false }));

/** `held`: cover the window now, start the animation on `releaseIntro`. */
export function playIntro({ held = false } = {}) {
  useIntro.setState((s) => ({ run: s.run + 1, held }));
}

export function releaseIntro() {
  useIntro.setState({ held: false });
}

/** Resolves once the intro has had a frame to cover the window. */
export function introCovering(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
}

/** Whether the page has the whole screen (sizes in CSS pixels, a pixel of rounding allowed). */
function fillsScreen() {
  return Math.abs(window.innerWidth - screen.width) <= 1 && Math.abs(window.innerHeight - screen.height) <= 1;
}

/** Resolves once the page has the screen's size: going fullscreen resizes
 * the window in steps, and not right when asked. Gives up after `cap` ms
 * (fullscreen refused, or a size that never matches exactly). */
export function fullscreenSized(target: EventTarget = window, full: () => boolean = fillsScreen, cap = 1500): Promise<void> {
  return new Promise((resolve) => {
    const finish = () => {
      clearTimeout(limit);
      target.removeEventListener("resize", check);
      resolve();
    };
    const check = () => {
      if (full()) finish();
    };
    const limit = setTimeout(finish, cap);
    target.addEventListener("resize", check);
    check();
  });
}
