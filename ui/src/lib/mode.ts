// Flick Frame: TV mode sharing all data with the desktop layout. It scales the
// UI (root font size), swaps the sidebar for a tvOS tab bar, hides the cursor
// when idle and goes fullscreen.
import { create } from "zustand";
import { api } from "@/ipc/api";
import { introCovering, playIntro } from "@/lib/intro";

export const useMode = create<{ frame: boolean }>(() => ({ frame: false }));

let cursorTimer: number | undefined;
function showCursorThenHide() {
  const root = document.documentElement;
  delete root.dataset.cursor;
  window.clearTimeout(cursorTimer);
  if (useMode.getState().frame) cursorTimer = window.setTimeout(() => (root.dataset.cursor = "hidden"), 2500);
}
window.addEventListener("mousemove", showCursorThenHide, { passive: true });

export async function setFrame(on: boolean) {
  useMode.setState({ frame: on });
  document.documentElement.classList.toggle("frame", on);
  showCursorThenHide();
  await api.setFullscreen(on).catch(() => undefined);
}

/** The sidebar button, the tab bar and the Menu key. Entering Flick Frame
 * plays the intro first and switches underneath it, so the fullscreen
 * resize and the new layout are never seen half done. */
export async function toggleFrame() {
  const on = !useMode.getState().frame;
  if (on) {
    playIntro();
    await introCovering();
  }
  await setFrame(on);
}
