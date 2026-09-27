// Plays the Flick intro over the app (App renders it): at start-up, and on
// demand while the app changes underneath (entering Flick Frame).
import { create } from "zustand";

/** Bumped on each request; App shows one intro per value. */
export const useIntro = create<{ run: number }>(() => ({ run: 0 }));

export function playIntro() {
  useIntro.setState((s) => ({ run: s.run + 1 }));
}

/** Resolves once the intro has had a frame to cover the window. */
export function introCovering(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
}
