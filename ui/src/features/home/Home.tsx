// Home: a full-bleed featured carousel, then the server's shelves. The
// carousel advances on its own until it has focus; with focus, left/right
// past the edge buttons page through it (tvOS top shelf).
import { useQuery } from "@tanstack/react-query";
import { Info, Play, Plus, RotateCw } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { Button } from "@/components/tv/Button";
import { detailPath } from "@/components/tv/Card";
import { EmptyState, Notice } from "@/components/tv/Feedback";
import { FlickMark } from "@/components/tv/FlickMark";
import { HeroBackdrop, MetaLine, TitleArt } from "@/components/tv/Hero";
import { Shelf } from "@/components/tv/Shelf";
import { api, asError } from "@/ipc/api";
import type { HomeRow } from "@/ipc/bindings/HomeRow";
import type { MediaItem } from "@/ipc/bindings/MediaItem";
import { ambientFor } from "@/lib/ambient";
import { remaining } from "@/lib/format";
import { enter, pillSpring } from "@/lib/motion";
import { useSettings } from "@/lib/settings";
import { FocusGroup, Screen } from "@/nav/Focusable";
import { onAction } from "@/nav/input";
import { serversQuery } from "@/shell/navItems";
import { isPlayable, playPath } from "../player/route";

const SLIDE_MS = 9000;
const MAX_SLIDES = 6;

function rowShape(row: HomeRow): "poster" | "thumb" {
  return row.kind.type === "continueWatching" || row.kind.type === "nextUp" ? "thumb" : "poster";
}

function rowId(row: HomeRow, i: number) {
  return `${row.kind.type}-${i}`;
}

/** Featured titles: discovery rows first, anything with a backdrop. */
function pickFeatured(rows: HomeRow[]): MediaItem[] {
  const order = ["recommended", "popular", "recentlyAdded", "nextUp", "continueWatching"];
  const sorted = [...rows].sort((a, b) => {
    const ia = order.indexOf(a.kind.type);
    const ib = order.indexOf(b.kind.type);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib);
  });
  const seen = new Set<string>();
  const out: MediaItem[] = [];
  for (const row of sorted) {
    for (const item of row.items) {
      if (!item.images.backdrop || seen.has(item.id)) continue;
      seen.add(item.id);
      out.push(item);
      if (out.length >= MAX_SLIDES) return out;
    }
  }
  return out;
}

function Featured({ items }: { items: MediaItem[] }) {
  const navigate = useNavigate();
  const [index, setIndex] = useState(0);
  const [focusInside, setFocusInside] = useState(false);
  const [hover, setHover] = useState(false);
  const item = items[index % items.length]!;
  const buttons = useRef<HTMLDivElement>(null);

  const go = (step: number) => setIndex((i) => (i + step + items.length) % items.length);

  // Auto-advance while the user is elsewhere.
  useEffect(() => {
    if (focusInside || hover || items.length < 2) return;
    const t = window.setTimeout(() => go(1), SLIDE_MS);
    return () => window.clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [index, focusInside, hover, items.length]);

  // The featured title sets the mood while it is being looked at.
  useEffect(() => {
    if (focusInside || hover) ambientFor(item);
  }, [item, focusInside, hover]);

  // Left/right beyond the first/last button pages the carousel.
  useEffect(() => {
    if (!focusInside || items.length < 2) return;
    return onAction((a) => {
      if (a.type !== "move" || (a.dir !== "left" && a.dir !== "right")) return false;
      const list = Array.from(buttons.current?.querySelectorAll<HTMLElement>("[data-tv]") ?? []);
      const at = list.indexOf(document.activeElement as HTMLElement);
      if (a.dir === "left" && at === 0) {
        go(-1);
        return true;
      }
      if (a.dir === "right" && at === list.length - 1) {
        go(1);
        return true;
      }
      return false;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusInside, items.length]);

  const resuming = item.user.positionMs > 0;
  const playable = isPlayable(item);

  return (
    <section
      className="relative flex min-h-[max(34rem,74vh)] flex-col justify-end"
      aria-roledescription="carousel"
      aria-label="Featured"
      onPointerEnter={() => setHover(true)}
      onPointerLeave={() => setHover(false)}
    >
      <HeroBackdrop image={item.images.backdrop} />
      <div className="relative flex flex-col gap-5 px-[var(--gutter)] pb-16">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={item.id}
            initial={{ opacity: 0, x: 24 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -16 }}
            transition={enter}
            className="flex max-w-2xl flex-col gap-4"
          >
            <TitleArt item={item} />
            <MetaLine item={item} />
            {item.overview && <p className="line-clamp-3 max-w-xl text-[1.0625rem] leading-relaxed text-white/80 text-pretty">{item.overview}</p>}
          </motion.div>
        </AnimatePresence>
        <div className="flex items-center justify-between gap-6">
          <div ref={buttons}>
            <FocusGroup focusKey="featured" onFocusWithin={setFocusInside} className="flex items-center gap-3">
              {playable ? (
                <Button variant="primary" size="lg" icon={Play} iconFilled onClick={() => navigate(playPath(item.id, resuming ? item.user.positionMs : 0))}>
                  {resuming ? `Resume · ${remaining(item)}` : "Play"}
                </Button>
              ) : (
                <Button variant="primary" size="lg" icon={Plus} onClick={() => navigate(detailPath(item.id))}>
                  View
                </Button>
              )}
              <Button size="lg" icon={Info} onClick={() => navigate(detailPath(item.id))}>
                Details
              </Button>
            </FocusGroup>
          </div>
          {items.length > 1 && (
            <div className="flex items-center gap-2" role="tablist" aria-label="Featured titles">
              {items.map((it, i) => (
                <button
                  key={it.id}
                  type="button"
                  tabIndex={-1}
                  role="tab"
                  aria-selected={i === index}
                  aria-label={it.title}
                  onClick={() => setIndex(i)}
                  className="relative h-2 w-2 cursor-pointer rounded-full bg-white/30 transition-[width] duration-500 ease-apple data-[on=true]:w-7"
                  data-on={i === index}
                >
                  {i === index && <motion.span layoutId="featured-dot" className="absolute inset-0 rounded-full bg-white" transition={pillSpring} />}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
    </section>
  );
}

export function Home() {
  const navigate = useNavigate();
  const settings = useSettings();
  const home = useQuery({ queryKey: ["home"], queryFn: () => api.home() });
  const servers = useQuery(serversQuery);
  const rows = home.data?.data ?? [];
  const featured = useMemo(() => pickFeatured(rows), [rows]);

  // Seed the ambience with the first featured title.
  useEffect(() => {
    const first = featured[0] ?? rows[0]?.items[0];
    if (first) ambientFor(first);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [home.data]);

  if (servers.data?.length === 0) {
    return (
      <Screen>
        <EmptyState
          title="Connect a server to start"
          icon={<FlickMark className="h-16 w-auto drop-shadow-[0_12px_40px_rgb(255_255_255/0.18)]" />}
          actions={
            <Button variant="primary" size="lg" icon={Plus} autoFocus onClick={() => navigate("/servers")}>
              Add a server
            </Button>
          }
        >
          Flick plays your Jellyfin and Plex libraries. Add as many servers as you like; they appear here as one collection.
        </EmptyState>
      </Screen>
    );
  }

  if (home.error) {
    return (
      <Screen>
        <EmptyState
          title="Could not load Home"
          actions={
            <Button variant="primary" icon={RotateCw} autoFocus onClick={() => void home.refetch()}>
              Try again
            </Button>
          }
        >
          {asError(home.error).message}
        </EmptyState>
      </Screen>
    );
  }

  if (!home.data) return <HomeSkeleton />;

  return (
    <Screen ready className="pb-16">
      {featured.length > 0 ? <Featured items={featured} /> : <div className="h-[var(--page-top)]" />}
      {home.data.issues.length > 0 && settings?.notifications.serverOffline !== false && (
        <div className="px-[var(--gutter)] pb-4">
          <Notice tone="warn">{home.data.issues.map((i) => `${i.name}: ${i.error}`).join(" · ")}</Notice>
        </div>
      )}
      <div className="relative flex flex-col gap-2">
        {rows.map((row, i) => (
          <Shelf key={rowId(row, i)} id={rowId(row, i)} index={i} title={row.title} items={row.items} shape={rowShape(row)} />
        ))}
      </div>
    </Screen>
  );
}

function HomeSkeleton() {
  return (
    <div aria-busy className="flex flex-col gap-10">
      <div className="flex min-h-[max(34rem,74vh)] flex-col justify-end gap-5 px-[var(--gutter)] pb-16">
        <div className="h-16 w-96 animate-pulse rounded-2xl bg-white/[0.06]" />
        <div className="h-4 w-72 animate-pulse rounded-full bg-white/[0.06]" />
        <div className="flex gap-3">
          <div className="h-13 w-36 animate-pulse rounded-full bg-white/[0.08]" />
          <div className="h-13 w-32 animate-pulse rounded-full bg-white/[0.06]" />
        </div>
      </div>
      <div className="flex gap-[var(--card-gap)] px-[var(--gutter)]">
        {Array.from({ length: 7 }, (_, i) => (
          <div key={i} className="aspect-video w-[var(--thumb-w)] shrink-0 animate-pulse rounded-xl bg-white/[0.06]" />
        ))}
      </div>
    </div>
  );
}
