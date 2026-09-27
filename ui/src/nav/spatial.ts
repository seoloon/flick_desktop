// Spatial navigation engine: Norigin owns the focus tree (containers remember
// their last focused child, boundaries trap dialogs) and the geometry. Input
// comes from input.ts, never from Norigin's own key listener.
import {
  doesFocusableExist,
  GetBoundingClientRectAdapter,
  getCurrentFocusKey,
  init,
  navigateByDirection,
  setFocus,
  setKeyMap,
} from "@noriginmedia/norigin-spatial-navigation";

export type Direction = "up" | "down" | "left" | "right";

/** Focus key of the current screen's root container (see `Screen`). */
export const SCREEN_KEY = "screen";
export const NAV_KEY = "nav";

export function installSpatialNavigation() {
  init({
    // DOM focus follows Norigin focus, so Enter/A click native buttons,
    // text fields receive keystrokes and screen readers follow along.
    shouldFocusDOMNode: true,
    domNodeFocusOptions: { preventScroll: true },
    // Focused cards are scaled and rows scroll: measure real boxes.
    layoutAdapter: GetBoundingClientRectAdapter,
    distanceCalculationMethod: "center",
    throttle: 0,
  });
  // Disable Norigin's key listener: every code list empty.
  setKeyMap({ left: [], up: [], right: [], down: [], enter: [] });
}

export function moveFocus(dir: Direction) {
  const current = getCurrentFocusKey();
  if (!current || !doesFocusableExist(current)) {
    focusScreen();
    return;
  }
  void navigateByDirection(dir, {});
}

/** Puts focus on the current screen (its remembered or first element). */
export function focusScreen() {
  if (doesFocusableExist(SCREEN_KEY)) void setFocus(SCREEN_KEY);
}

export function focusKey(key: string) {
  if (doesFocusableExist(key)) void setFocus(key);
}

/** A/Enter on a focused element that is not a native button: click it. */
export function activate(): boolean {
  const el = document.activeElement as HTMLElement | null;
  if (!el || el === document.body || !el.hasAttribute("data-tv")) return false;
  el.click();
  return true;
}
