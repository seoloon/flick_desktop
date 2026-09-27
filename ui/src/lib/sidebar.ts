// Sidebar collapsed/expanded. A per-device convenience, so it lives in
// localStorage (guarded: the store can be unavailable). The width itself is
// the registered `--sidebar-w` CSS property, which animates.
import { create } from "zustand";

const KEY = "flick.sidebar.collapsed";

function read(): boolean {
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

function apply(collapsed: boolean) {
  if (collapsed) document.documentElement.dataset.sidebar = "collapsed";
  else delete document.documentElement.dataset.sidebar;
}

const initial = read();
apply(initial);

export const useSidebar = create<{ collapsed: boolean }>(() => ({ collapsed: initial }));

const EASE = "cubic-bezier(0.32, 0.72, 0, 1)";
const DURATION = 500;

/**
 * FLIP: the screen takes its new margin at once (a single layout), then
 * slides from where it was by transform, which the compositor animates
 * without relayout. Only the light sidebar actually changes width.
 */
function slideContent(update: () => void) {
  const main = document.querySelector("main");
  const before = main ? parseFloat(getComputedStyle(main).paddingLeft) : 0;
  update();
  if (!main || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const delta = before - parseFloat(getComputedStyle(main).paddingLeft);
  if (Math.abs(delta) < 1) return;
  main.animate([{ transform: `translateX(${delta}px)` }, { transform: "translateX(0)" }], { duration: DURATION, easing: EASE });
}

export function toggleSidebar() {
  const collapsed = !useSidebar.getState().collapsed;
  useSidebar.setState({ collapsed });
  slideContent(() => apply(collapsed));
  try {
    localStorage.setItem(KEY, collapsed ? "1" : "0");
  } catch {
    // Not persisted this time; the toggle still works.
  }
}
