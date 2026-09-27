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

/** Resolves once `target` has gone `quiet` ms without a resize (a window
 * going fullscreen grows in several steps), or after `cap` ms whatever. */
export function settled(target: EventTarget = window, quiet = 150, cap = 1000): Promise<void> {
  return new Promise((resolve) => {
    let timer: ReturnType<typeof setTimeout>;
    const finish = () => {
      clearTimeout(timer);
      clearTimeout(limit);
      target.removeEventListener("resize", wait);
      resolve();
    };
    const wait = () => {
      clearTimeout(timer);
      timer = setTimeout(finish, quiet);
    };
    const limit = setTimeout(finish, cap);
    target.addEventListener("resize", wait);
    wait();
  });
}
