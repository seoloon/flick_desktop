// Flick Frame: TV mode sharing all data with the desktop layout. It scales the
// UI (root font size), swaps the sidebar for a tvOS tab bar, hides the cursor
// when idle and goes fullscreen.
import { create } from "zustand";
import { api } from "@/ipc/api";
import { fullscreenSized, introCovering, playIntro, releaseIntro } from "@/lib/intro";

export const useMode = create<{ frame: boolean }>(() => ({ frame: false }));

let cursorTimer: number | undefined;
function showCursorThenHide() {
  const root = document.documentElement;
  delete root.dataset.cursor;
  window.clearTimeout(cursorTimer);
  if (useMode.getState().frame) cursorTimer = window.setTimeout(() => (root.dataset.cursor = "hidden"), 2500);
}
window.addEventListener("mousemove", showCursorThenHide, { passive: true });

/** `moveWindow: false` only switches the layout: the caller already moved the
 * window (Picture in Picture leaves and restores fullscreen by itself). */
export async function setFrame(on: boolean, moveWindow = true) {
  useMode.setState({ frame: on });
  document.documentElement.classList.toggle("frame", on);
  showCursorThenHide();
  if (moveWindow) await api.setFullscreen(on).catch(() => undefined);
}

/** The sidebar button, the tab bar and the Menu key. Entering Flick Frame
 * covers the window, switches underneath and starts the intro only once
 * the window has finished growing: the fullscreen resize comes in steps,
 * and an animation already playing would be seen stretching with it. */
export async function toggleFrame() {
  const on = !useMode.getState().frame;
  if (!on) return setFrame(false);
  playIntro({ held: true });
  await introCovering();
  await setFrame(true);
  await fullscreenSized();
  await introCovering();
  releaseIntro();
}
