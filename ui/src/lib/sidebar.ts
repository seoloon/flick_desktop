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

export function toggleSidebar() {
  const collapsed = !useSidebar.getState().collapsed;
  useSidebar.setState({ collapsed });
  // The screen takes its new margin at once while the sidebar, which floats
  // over it, glides alone. Sliding the screen too would make every frosted
  // layer re-blur moving content for the whole animation: WebKit cannot.
  apply(collapsed);
  try {
    localStorage.setItem(KEY, collapsed ? "1" : "0");
  } catch {
    // Not persisted this time; the toggle still works.
  }
}
