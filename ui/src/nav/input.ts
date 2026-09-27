// Input routing: keyboard, gamepads (Gamepad API) and TV remotes (which arrive
// as arrow/Enter/Escape/media keys) all become one set of actions. Screens
// register handlers for actions they own (the player takes Space and seeking);
// everything else falls back to spatial navigation and history.
//
// Norigin's own key listener is disabled (see spatial.ts): it calls
// preventDefault on arrows and Enter even while paused, which breaks text
// fields, and only reports arrows to the focused leaf. Routing input here
// keeps one priority stack that any component can join.

import { create } from "zustand";
import { activate, moveFocus, type Direction } from "./spatial";

export type Action =
  | { type: "move"; dir: Direction }
  | { type: "activate" }
  | { type: "back" }
  | { type: "playPause" }
  | { type: "seek"; seconds: number }
  | { type: "menu" };

/** How the user is driving the UI: focus visuals only show for `keys`. */
export type Modality = "keys" | "pointer";
export const useModality = create<{ modality: Modality }>(() => ({ modality: "pointer" }));

function setModality(m: Modality) {
  if (useModality.getState().modality === m) return;
  useModality.setState({ modality: m });
  document.documentElement.dataset.modality = m;
}

type Handler = (a: Action) => boolean;
const handlers: Handler[] = [];

/** Registers a handler with priority over the defaults (latest wins); returns an unregister fn. */
export function onAction(h: Handler): () => void {
  handlers.unshift(h);
  return () => {
    const i = handlers.indexOf(h);
    if (i >= 0) handlers.splice(i, 1);
  };
}

let backFallback: () => void = () => history.back();
export function setBackFallback(fn: () => void) {
  backFallback = fn;
}

export function dispatch(a: Action): boolean {
  for (const h of [...handlers]) if (h(a)) return true;
  switch (a.type) {
    case "move":
      moveFocus(a.dir);
      return true;
    case "activate":
      return activate();
    case "back":
      backFallback();
      return true;
    default:
      return false;
  }
}

function isTextInput(el: Element | null): boolean {
  if (!el) return false;
  if (el.tagName === "TEXTAREA" || el.tagName === "SELECT") return true;
  if (el.tagName !== "INPUT") return false;
  const type = (el as HTMLInputElement).type;
  return type !== "range" && type !== "checkbox" && type !== "radio" && type !== "button";
}

const KEYS: Record<string, Action> = {
  ArrowUp: { type: "move", dir: "up" },
  ArrowDown: { type: "move", dir: "down" },
  ArrowLeft: { type: "move", dir: "left" },
  ArrowRight: { type: "move", dir: "right" },
  Enter: { type: "activate" },
  Escape: { type: "back" },
  Backspace: { type: "back" },
  BrowserBack: { type: "back" },
  GoBack: { type: "back" },
  MediaPlayPause: { type: "playPause" },
  " ": { type: "playPause" },
  MediaFastForward: { type: "seek", seconds: 10 },
  MediaRewind: { type: "seek", seconds: -10 },
  ContextMenu: { type: "menu" },
};

// Held arrows repeat at the OS rate (~30 Hz); cap it so focus animations and
// smooth scrolling keep up.
const REPEAT_MIN_MS = 70;
let lastRepeat = 0;

export function installKeyboard() {
  window.addEventListener(
    "keydown",
    (e) => {
      const action = KEYS[e.key];
      if (!action || e.ctrlKey || e.metaKey || e.altKey) return;
      const active = document.activeElement;
      const typing = isTextInput(active);
      // Text fields keep their keys; only vertical moves and Escape leave them.
      if (typing && !(action.type === "back" && e.key === "Escape") && !(action.type === "move" && (action.dir === "up" || action.dir === "down"))) return;
      // Sliders keep left/right.
      if (active instanceof HTMLInputElement && active.type === "range" && action.type === "move" && (action.dir === "left" || action.dir === "right")) return;
      // Enter on a native button or link already clicks.
      if (action.type === "activate" && (active?.tagName === "BUTTON" || active?.tagName === "A")) {
        setModality("keys");
        return;
      }
      if (action.type === "move") {
        setModality("keys");
        const now = performance.now();
        if (e.repeat && now - lastRepeat < REPEAT_MIN_MS) {
          e.preventDefault();
          return;
        }
        lastRepeat = now;
      }
      if (dispatch(action)) {
        e.preventDefault();
        e.stopPropagation();
      }
    },
    { capture: true },
  );
  window.addEventListener("pointermove", (e) => (e.movementX || e.movementY) && setModality("pointer"), { passive: true });
  window.addEventListener("pointerdown", () => setModality("pointer"), { passive: true });
  window.addEventListener("wheel", () => setModality("pointer"), { passive: true });
}

// ------------------------------------------------------------------ gamepad

const REPEAT_DELAY = 380;
const REPEAT_RATE = 110;

export function installGamepad(deadzone: () => number, swapConfirm: () => boolean, enabled: () => boolean) {
  const held = new Map<string, number>(); // key -> next fire time
  const fire = (key: string, pressed: boolean, action: Action, now: number) => {
    if (!pressed) {
      held.delete(key);
      return;
    }
    const next = held.get(key);
    if (next === undefined) {
      setModality("keys");
      dispatch(action);
      held.set(key, now + REPEAT_DELAY);
    } else if (now >= next && action.type === "move") {
      dispatch(action);
      held.set(key, now + REPEAT_RATE);
    }
  };
  const loop = (now: number) => {
    const pads = enabled() ? (navigator.getGamepads?.() ?? []) : [];
    for (const pad of pads) {
      if (!pad || pad.mapping !== "standard") continue;
      const b = (i: number) => pad.buttons[i]?.pressed ?? false;
      const dz = deadzone();
      const ax = pad.axes[0] ?? 0;
      const ay = pad.axes[1] ?? 0;
      const [confirm, cancel] = swapConfirm() ? [1, 0] : [0, 1];
      fire(`${pad.index}:up`, b(12) || ay < -dz, { type: "move", dir: "up" }, now);
      fire(`${pad.index}:down`, b(13) || ay > dz, { type: "move", dir: "down" }, now);
      fire(`${pad.index}:left`, b(14) || ax < -dz, { type: "move", dir: "left" }, now);
      fire(`${pad.index}:right`, b(15) || ax > dz, { type: "move", dir: "right" }, now);
      fire(`${pad.index}:a`, b(confirm), { type: "activate" }, now);
      fire(`${pad.index}:b`, b(cancel), { type: "back" }, now);
      fire(`${pad.index}:start`, b(9), { type: "playPause" }, now);
      fire(`${pad.index}:lb`, b(4), { type: "seek", seconds: -10 }, now);
      fire(`${pad.index}:rb`, b(5), { type: "seek", seconds: 10 }, now);
      fire(`${pad.index}:menu`, b(8), { type: "menu" }, now);
    }
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
}
