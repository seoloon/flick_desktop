// Media cards. Shape follows content: 2:3 posters for titles, 16:9 for
// episodes and resumable items (with progress). Focus is the one spectacular
// thing: the card lifts, casts a deep shadow and catches a specular sheen;
// under a pointer it also tilts toward the cursor (tvOS parallax).
import { Check } from "lucide-react";
import { motion, useMotionTemplate, useMotionValue, useSpring } from "motion/react";
import { type PointerEvent, useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import type { ItemRef } from "@/ipc/bindings/ItemRef";
import type { MediaItem } from "@/ipc/bindings/MediaItem";
import { imageUrl } from "@/ipc/images";
import type { ImageRef } from "@/ipc/bindings/ImageRef";
import type { ImageSize } from "@/ipc/bindings/ImageSize";
import { ambientFor } from "@/lib/ambient";
import { episodeLabel, progress } from "@/lib/format";
import { openItemMenu, useOverridden } from "@/lib/itemMenu";
import { focusSpring } from "@/lib/motion";
import { useSources } from "@/lib/servers";
import { cn } from "@/lib/utils";
import { ServerBadge } from "./ServerBadge";
import { useTv } from "@/nav/Focusable";
import { onStick } from "@/nav/input";
import { navSection } from "@/shell/navItems";

export function detailPath(id: ItemRef) {
  return `/item/${encodeURIComponent(id)}`;
}

export function Artwork({ image, size, alt, className }: { image: ImageRef | null | undefined; size: ImageSize; alt: string; className?: string }) {
  const src = imageUrl(image, size);
  const [loaded, setLoaded] = useState(false);
  const [failed, setFailed] = useState(false);
  return (
    <div className={cn("absolute inset-0 bg-white/[0.06]", className)}>
      {src && !failed ? (
        <img
          src={src}
          alt=""
          loading="lazy"
          decoding="async"
          onLoad={() => setLoaded(true)}
          onError={() => setFailed(true)}
          className={cn("size-full object-cover transition-opacity duration-500 ease-apple", loaded ? "opacity-100" : "opacity-0")}
        />
      ) : (
        <span className="absolute inset-0 grid place-items-center p-4 text-center font-heading text-lg font-semibold text-balance text-white/55">{alt}</span>
      )}
    </div>
  );
}

const MAX_TILT = 9;

type CardProps = {
  item: MediaItem;
  shape: "poster" | "thumb";
  focusKey?: string;
  /** Fill the grid cell instead of the shelf width. */
  fluid?: boolean;
  onSelect?: (item: MediaItem) => void;
};

export function MediaCard({ item: listed, shape, focusKey, fluid, onSelect }: CardProps) {
  const item = useOverridden(listed);
  const navigate = useNavigate();
  const { pathname, search, state } = useLocation();
  const tv = useTv<HTMLButtonElement>({ focusKey, onFocused: () => ambientFor(item) });
  const [hover, setHover] = useState(false);
  const lifted = tv.showFocus || hover;

  // Tilt and sheen follow the pointer; with a remote they rest at a gentle
  // top-left light, like the Apple TV idle state.
  const rx = useMotionValue(0);
  const ry = useMotionValue(0);
  const srx = useSpring(rx, { stiffness: 300, damping: 26, mass: 0.5 });
  const sry = useSpring(ry, { stiffness: 300, damping: 26, mass: 0.5 });
  const lx = useSpring(30, { stiffness: 200, damping: 30 });
  const ly = useSpring(0, { stiffness: 200, damping: 30 });
  const sheen = useMotionTemplate`radial-gradient(90% 70% at ${lx}% ${ly}%, rgb(255 255 255 / 0.09), rgb(255 255 255 / 0.02) 45%, transparent 70%)`;

  const onMove = (e: PointerEvent<HTMLButtonElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const px = (e.clientX - r.left) / r.width;
    const py = (e.clientY - r.top) / r.height;
    ry.set((px * 2 - 1) * MAX_TILT);
    rx.set(-(py * 2 - 1) * MAX_TILT);
    lx.set(px * 100);
    ly.set(py * 100);
  };
  // With a controller the left stick plays the pointer's part on the focused card.
  useEffect(() => {
    if (!tv.showFocus) return;
    const off = onStick((x, y) => {
      ry.set(x * MAX_TILT);
      rx.set(-y * MAX_TILT);
      lx.set(x ? 50 + x * 50 : 30);
      ly.set(y || x ? 50 + y * 50 : 0);
    });
    return () => {
      off();
      rx.set(0);
      ry.set(0);
      lx.set(30);
      ly.set(0);
    };
  }, [tv.showFocus, rx, ry, lx, ly]);
  const onLeave = () => {
    setHover(false);
    rx.set(0);
    ry.set(0);
    lx.set(30);
    ly.set(0);
  };

  const open = () => (onSelect ? onSelect(item) : navigate(detailPath(item.id), { state: { navSection: navSection(pathname, search, state) } }));
  const pct = progress(item);
  const [source] = useSources([item.id]);
  const thumb = shape === "thumb";
  const art = thumb ? (item.images.thumb ?? item.images.backdrop ?? item.images.poster) : item.images.poster;
  const title = thumb ? (item.episode?.seriesTitle ?? item.title) : item.title;
  const sub = item.episode ? `${episodeLabel(item)} · ${item.title}` : (item.year?.toString() ?? "");

  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      onClick={open}
      onContextMenu={(e) => {
        e.preventDefault();
        // From the keyboard (Shift+F10) the event has no cursor position: open on the card.
        const r = e.currentTarget.getBoundingClientRect();
        const keyboard = e.clientX === 0 && e.clientY === 0;
        openItemMenu(item, keyboard ? r.left + r.width / 2 : e.clientX, keyboard ? r.top + r.height / 2 : e.clientY);
      }}
      onPointerEnter={() => {
        setHover(true);
        ambientFor(item);
      }}
      onPointerMove={onMove}
      onPointerLeave={onLeave}
      aria-label={sub ? `${title}, ${sub}` : title}
      className={cn("group relative flex shrink-0 cursor-pointer flex-col gap-3 text-left scroll-mx-[var(--gutter)] scroll-mt-24 scroll-mb-28", fluid ? "w-full" : thumb ? "w-[var(--thumb-w)]" : "w-[var(--poster-w)]")}
      style={{ perspective: 900 }}
      animate={{ scale: tv.showFocus ? 1.1 : hover ? 1.04 : 1, zIndex: lifted ? 10 : 0 }}
      whileTap={{ scale: tv.showFocus ? 1.04 : 0.97 }}
      transition={focusSpring}
    >
      <motion.div
        className={cn("relative w-full overflow-hidden bg-white/[0.06]", thumb ? "aspect-video rounded-xl" : "aspect-[2/3] rounded-lg")}
        style={{ rotateX: srx, rotateY: sry, transformStyle: "preserve-3d" }}
        animate={{
          boxShadow: lifted ? "0 32px 56px -18px rgb(0 0 0 / 0.85), 0 0 0 1px rgb(255 255 255 / 0.10)" : "0 10px 24px -14px rgb(0 0 0 / 0.6), 0 0 0 1px rgb(255 255 255 / 0.06)",
        }}
        transition={focusSpring}
      >
        <Artwork image={art} size={thumb ? "large" : "card"} alt={title} />
        {/* Specular sheen */}
        <motion.div aria-hidden className="pointer-events-none absolute inset-0" style={{ background: sheen }} animate={{ opacity: lifted ? 1 : 0 }} transition={{ duration: 0.25 }} />
        {source && (
          <motion.span
            className="glass absolute top-2 left-2 max-w-[calc(100%-3rem)] rounded-full px-2 py-0.5 text-[0.6875rem]"
            initial={false}
            animate={{ opacity: lifted ? 1 : 0, y: lifted ? 0 : -4 }}
            transition={{ duration: 0.2 }}
          >
            <ServerBadge server={source} />
          </motion.span>
        )}
        {item.user.played && (
          <span className="glass absolute top-2 right-2 grid size-6 place-items-center rounded-full text-white" aria-label="Watched">
            <Check className="size-3.5" strokeWidth={3} />
          </span>
        )}
        {pct > 0 && (
          <span className="absolute inset-x-3 bottom-3 h-1 overflow-hidden rounded-full bg-white/25 backdrop-blur">
            <span className="block h-full rounded-full bg-white" style={{ width: `${pct * 100}%` }} />
          </span>
        )}
      </motion.div>
      <motion.span className="flex min-w-0 flex-col gap-0.5 px-0.5" animate={{ y: tv.showFocus ? 6 : 0, opacity: tv.showFocus || hover ? 1 : 0.78 }} transition={focusSpring}>
        <span className="truncate text-[0.9375rem] leading-tight font-semibold text-white">{title}</span>
        {sub && <span className="truncate text-[0.8125rem] leading-tight text-muted-foreground">{sub}</span>}
      </motion.span>
    </motion.button>
  );
}

/** Non-interactive stand-in while a page of a library loads. */
export function CardPlaceholder({ shape, fluid }: { shape: "poster" | "thumb"; fluid?: boolean }) {
  return (
    <div className={cn("flex shrink-0 flex-col gap-3", fluid ? "w-full" : shape === "thumb" ? "w-[var(--thumb-w)]" : "w-[var(--poster-w)]")}>
      <div className={cn("w-full animate-pulse bg-white/[0.06]", shape === "thumb" ? "aspect-video rounded-xl" : "aspect-[2/3] rounded-lg")} />
      <div className="h-3 w-2/3 rounded-full bg-white/[0.06]" />
    </div>
  );
}
