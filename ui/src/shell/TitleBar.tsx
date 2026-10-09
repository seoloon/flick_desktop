// The window's title bar, drawn by the app.
//
// Windows/Linux: the window has no native decorations (Windows 11 still
// rounds its corners and draws the shadow, see `shadow` in tauri.conf.json).
// A transparent strip along the top drags the window (double-click
// maximizes) and a glass capsule holds minimize / maximize / close.
//
// macOS: removing the title bar would also remove the native window shape
// (rounded corners, shadow, resize behaviour). Instead the title bar is an
// *overlay* (tauri.macos.conf.json) and the native traffic lights are hidden
// (`hide_traffic_lights` in app/src/main.rs). The same glass capsule as on
// Windows holds redrawn traffic lights, in the same top right spot.
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minimize2, Minus, Square, X } from "lucide-react";
import { motion } from "motion/react";
import { type CSSProperties, type MouseEvent, type ReactNode, type WheelEvent, useEffect, useState } from "react";
import { useMode } from "@/lib/mode";
import { cn } from "@/lib/utils";

export const isMac = /Mac/.test(navigator.userAgent);

function useMaximized() {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    let alive = true;
    const check = () => void win.isMaximized().then((m) => alive && setMaximized(m), () => undefined);
    check();
    void win.onResized(check).then((u) => (alive ? (unlisten = u) : u()));
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
  return maximized;
}

function Control({ label, onClick, danger, children }: { label: string; onClick: () => void; danger?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      tabIndex={-1}
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        "grid size-7 cursor-default place-items-center rounded-full text-white/75 transition-colors duration-150 [&_svg]:size-3.5",
        danger ? "hover:bg-[#e5484d] hover:text-white" : "hover:bg-white/16 hover:text-white",
      )}
    >
      {children}
    </button>
  );
}

function WindowControls({ hidden }: { hidden: boolean }) {
  const maximized = useMaximized();
  const win = getCurrentWindow();
  return (
    <motion.div
      className="glass fixed top-2.5 right-2.5 z-50 flex items-center gap-0.5 rounded-full p-1"
      animate={{ opacity: hidden ? 0 : 1, y: hidden ? -8 : 0 }}
      transition={{ duration: 0.25 }}
      style={{ pointerEvents: hidden ? "none" : "auto" }}
    >
      <Control label="Minimize" onClick={() => void win.minimize()}>
        <Minus strokeWidth={2.2} />
      </Control>
      <Control label={maximized ? "Restore" : "Maximize"} onClick={() => void win.toggleMaximize()}>
        {maximized ? <Copy strokeWidth={2} className="-scale-x-100" /> : <Square strokeWidth={2.2} />}
      </Control>
      <Control label="Close" danger onClick={() => void win.close()}>
        <X strokeWidth={2.4} />
      </Control>
    </motion.div>
  );
}

function useWindowState() {
  const [state, setState] = useState({ focused: true, fullscreen: false });
  useEffect(() => {
    const win = getCurrentWindow();
    const unlisten: (() => void)[] = [];
    let alive = true;
    const keep = (u: () => void) => (alive ? unlisten.push(u) : u());
    const check = () => void win.isFullscreen().then((fullscreen) => alive && setState((s) => ({ ...s, fullscreen })), () => undefined);
    check();
    void win.onResized(check).then(keep);
    void win.onFocusChanged(({ payload }) => alive && setState((s) => ({ ...s, focused: payload }))).then(keep);
    return () => {
      alive = false;
      unlisten.forEach((u) => u());
    };
  }, []);
  return state;
}

const glyph = { fill: "none", stroke: "currentColor", strokeWidth: 1.4, strokeLinecap: "round", strokeLinejoin: "round" } as const;

function Light({ label, color, onClick, focused, children }: { label: string; color: string; onClick: (e: MouseEvent) => void; focused: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      tabIndex={-1}
      aria-label={label}
      title={label}
      onClick={onClick}
      style={{ "--light": color } as CSSProperties}
      className={cn(
        "grid size-3 cursor-default place-items-center rounded-full text-black/60 shadow-[inset_0_0_0_0.5px_rgb(0_0_0/0.22)] transition-colors duration-150 active:brightness-75",
        focused ? "bg-(--light)" : "bg-white/25 group-hover/lights:bg-(--light)",
      )}
    >
      <svg viewBox="0 0 8 8" className="size-2 opacity-0 transition-opacity duration-100 group-hover/lights:opacity-100">
        {children}
      </svg>
    </button>
  );
}

function MacControls({ hidden }: { hidden: boolean }) {
  const { focused, fullscreen } = useWindowState();
  const win = getCurrentWindow();
  const off = hidden;
  // Native fullscreen has no traffic lights: one button gets back out.
  if (fullscreen) {
    return (
      <motion.div
        className="glass fixed top-2.5 right-2.5 z-50 flex items-center rounded-full p-1"
        animate={{ opacity: off ? 0 : 1, y: off ? -8 : 0 }}
        transition={{ duration: 0.25 }}
        style={{ pointerEvents: off ? "none" : "auto" }}
      >
        <Control label="Exit full screen" onClick={() => void win.setFullscreen(false)}>
          <Minimize2 strokeWidth={2.2} />
        </Control>
      </motion.div>
    );
  }
  return (
    <motion.div
      className="glass group/lights fixed top-2.5 right-2.5 z-50 flex h-9 items-center gap-2 rounded-full px-3.5"
      animate={{ opacity: off ? 0 : 1, y: off ? -8 : 0 }}
      transition={{ duration: 0.25 }}
      style={{ pointerEvents: off ? "none" : "auto" }}
    >
      <Light label="Close" color="#ff5f57" focused={focused} onClick={() => void win.close()}>
        <path {...glyph} d="M1.8 1.8l4.4 4.4M6.2 1.8L1.8 6.2" />
      </Light>
      <Light label="Minimize" color="#febc2e" focused={focused} onClick={() => void win.minimize()}>
        <path {...glyph} d="M1.4 4h5.2" />
      </Light>
      <Light
        label="Full screen"
        color="#28c840"
        focused={focused}
        // Like the native green button: full screen, or zoom with Option held.
        onClick={(e) => void (e.altKey ? win.toggleMaximize() : win.setFullscreen(true))}
      >
        <path fill="currentColor" stroke="none" d="M1.5 1.5h3.6L1.5 5.1zM6.5 6.5H2.9l3.6-3.6z" />
      </Light>
    </motion.div>
  );
}

/**
 * `hidden` fades the controls (the player hides them with its chrome).
 * `onWheel` lets the strip pass wheel scrolling to the screen underneath.
 */
export function TitleBar({ hidden = false, onWheel }: { hidden?: boolean; onWheel?: (e: WheelEvent<HTMLDivElement>) => void }) {
  const frame = useMode((s) => s.frame);
  if (frame) return null; // Flick Frame is fullscreen: no window chrome.
  return (
    <>
      <div data-tauri-drag-region onWheel={onWheel} className="fixed inset-x-0 top-0 z-40 h-11" />
      {isMac ? <MacControls hidden={hidden} /> : <WindowControls hidden={hidden} />}
    </>
  );
}
