// The window's title bar, drawn by the app.
//
// Windows/Linux: the window has no native decorations (Windows 11 still
// rounds its corners and draws the shadow, see `shadow` in tauri.conf.json).
// A transparent strip along the top drags the window (double-click
// maximizes) and a glass capsule holds minimize / maximize / close.
//
// macOS: removing the title bar would also remove the native window shape
// (rounded corners, shadow, resize behaviour). Instead the title bar is an
// *overlay* (tauri.macos.conf.json): content runs underneath it and the real
// traffic lights are moved into the sidebar header. Only the drag strip is
// drawn here.
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { motion } from "motion/react";
import { type ReactNode, type WheelEvent, useEffect, useState } from "react";
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
      {!isMac && <WindowControls hidden={hidden} />}
    </>
  );
}
