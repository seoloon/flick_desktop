// Right-click menu for a media item: only what a viewer needs (play, resume,
// watched, favourite, jump to the series). Library admin actions live in the
// server's own UI. It is modal for input: arrows, Enter and Back (keyboard,
// remote or controller) drive it, and everything else waits until it closes.
import { Check, Heart, Info, type LucideIcon, Play, RotateCcw, Tv } from "lucide-react";
import { motion } from "motion/react";
import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { toast } from "sonner";
import { isPlayable, playPath } from "@/features/player/route";
import type { MediaItem } from "@/ipc/bindings/MediaItem";
import { remaining } from "@/lib/format";
import { closeItemMenu, setItemFlag, useItemMenu } from "@/lib/itemMenu";
import { panelSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { onAction, useModality } from "@/nav/input";
import { navSection } from "@/shell/navItems";
import { detailPath } from "./Card";
import { errorText } from "@/lib/errors";

type Entry = { key: string; icon: LucideIcon; label: string; filled?: boolean; run: () => void; separated?: boolean };

const EDGE = 12;

export function ItemMenu() {
  const target = useItemMenu((s) => s.target);
  return target ? <Menu key={`${target.item.id}:${target.x}:${target.y}`} {...target} /> : null;
}

function Menu({ item, x, y }: { item: MediaItem; x: number; y: number }) {
  const navigate = useNavigate();
  const { pathname, search, state } = useLocation();
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const [active, setActive] = useState(() => (useModality.getState().modality === "keys" ? 0 : -1));

  const resuming = item.user.positionMs > 0;
  const series = item.episode?.series ?? null;
  const toDetail = (id: string) => navigate(detailPath(id), { state: { navSection: navSection(pathname, search, state) } });
  const flag = (what: "played" | "favorite") => () => void setItemFlag(item, what).catch((e) => toast.error(errorText(e)));

  const entries: Entry[] = [];
  if (isPlayable(item)) {
    entries.push({ key: "play", icon: Play, filled: true, label: resuming ? `Resume · ${remaining(item)}` : "Play", run: () => navigate(playPath(item.id, resuming ? item.user.positionMs : 0)) });
    if (resuming) entries.push({ key: "restart", icon: RotateCcw, label: "Start over", run: () => navigate(playPath(item.id, 0)) });
    entries.push({ key: "details", icon: Info, label: "Details", run: () => toDetail(item.id) });
  } else {
    entries.push({ key: "open", icon: Info, label: "Open", run: () => toDetail(item.id) });
  }
  if (series) entries.push({ key: "series", icon: Tv, label: "Go to series", run: () => toDetail(series) });
  entries.push({ key: "played", icon: Check, label: item.user.played ? "Mark as unwatched" : "Mark as watched", run: flag("played"), separated: true });
  entries.push({ key: "favorite", icon: Heart, filled: item.user.favorite, label: item.user.favorite ? "Remove from favourites" : "Add to favourites", run: flag("favorite") });

  // Keep the menu on screen: open at the cursor, flip or slide when it would overflow.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const left = x + width + EDGE > innerWidth ? Math.max(EDGE, x - width) : x;
    const top = y + height + EDGE > innerHeight ? Math.max(EDGE, innerHeight - height - EDGE) : y;
    setPos({ left, top });
  }, [x, y]);

  // Modal for input: consume everything while open.
  const latest = useRef({ entries, active });
  latest.current = { entries, active };
  useEffect(() => {
    const off = onAction((a) => {
      const { entries: list, active: i } = latest.current;
      if (a.type === "move" && (a.dir === "up" || a.dir === "down")) {
        const step = a.dir === "down" ? 1 : -1;
        setActive(i < 0 ? (step > 0 ? 0 : list.length - 1) : (i + step + list.length) % list.length);
      } else if (a.type === "activate") {
        const entry = list[i];
        if (!entry) return true;
        closeItemMenu();
        entry.run();
      } else if (a.type === "back" || a.type === "menu") {
        closeItemMenu();
      }
      return true;
    });
    // A click elsewhere, scrolling or resizing dismisses it, as a native menu would.
    const outside = (e: Event) => !ref.current?.contains(e.target as Node) && closeItemMenu();
    const dismiss = () => closeItemMenu();
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("contextmenu", outside, true);
    window.addEventListener("wheel", dismiss, { passive: true });
    window.addEventListener("resize", dismiss);
    window.addEventListener("blur", dismiss);
    return () => {
      off();
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("contextmenu", outside, true);
      window.removeEventListener("wheel", dismiss);
      window.removeEventListener("resize", dismiss);
      window.removeEventListener("blur", dismiss);
    };
  }, []);

  // Leaving the screen closes it.
  useEffect(() => closeItemMenu, [pathname]);

  return (
    <motion.div
      ref={ref}
      role="menu"
      aria-label={item.title}
      initial={{ opacity: 0, scale: 0.94 }}
      animate={{ opacity: pos ? 1 : 0, scale: pos ? 1 : 0.94 }}
      transition={panelSpring}
      style={{ left: pos?.left ?? x, top: pos?.top ?? y, transformOrigin: "top left" }}
      onContextMenu={(e) => e.preventDefault()}
      className="glass-strong fixed z-[60] flex min-w-60 flex-col gap-0.5 rounded-2xl p-1.5 shadow-[0_28px_60px_-16px_rgb(0_0_0/0.8)] ring-1 ring-white/10"
    >
      {entries.map((e, i) => (
        <Fragment key={e.key}>
          {e.separated && <div role="separator" className="mx-2 my-1 h-px bg-white/10" />}
          <button
            type="button"
            role="menuitem"
            onPointerMove={() => active !== i && setActive(i)}
            onClick={() => {
              closeItemMenu();
              e.run();
            }}
            className={cn(
              "flex h-10 w-full cursor-pointer items-center gap-3 rounded-xl px-3 text-left text-[0.9375rem] font-medium transition-colors duration-100",
              active === i ? "bg-white text-black" : "text-white/90",
            )}
          >
            <e.icon className={cn("size-[1.125rem] shrink-0", e.filled && "fill-current")} />
            <span className="truncate">{e.label}</span>
          </button>
        </Fragment>
      ))}
    </motion.div>
  );
}
