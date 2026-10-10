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
import { api } from "@/ipc/api";
import type { HomeRow } from "@/ipc/bindings/HomeRow";
import type { MediaItem } from "@/ipc/bindings/MediaItem";
import { ambientFor } from "@/lib/ambient";
import { remaining } from "@/lib/format";
import { enter } from "@/lib/motion";
import { useSettings } from "@/lib/settings";
import { FocusGroup, Screen } from "@/nav/Focusable";
import { onAction, useModality } from "@/nav/input";
import { serversQuery } from "@/shell/navItems";
import { isPlayable, playPath } from "../player/route";
import { errorText } from "@/lib/errors";

const SLIDE_MS = 9000;
const MAX_SLIDES = 6;

function rowShape(row: HomeRow): "poster" | "thumb" {
  return row.kind.type === "continueWatching" || row.kind.type === "nextUp" ? "thumb" : "poster";
}

function rowId(row: HomeRow, i: number) {
  return `${row.kind.type}-${i}`;
}

// One draw per app session: coming back to Home keeps the same lineup,
// the next launch brings a new one.
const SESSION_SEED = Math.floor(Math.random() * 2 ** 31);

/** Small seeded PRNG (mulberry32), so a lineup is stable for its seed. */
function random(seed: number) {
  let t = seed;
  return () => {
    t = (t + 0x6d2b79f5) | 0;
    let r = Math.imul(t ^ (t >>> 15), 1 | t);
    r = (r + Math.imul(r ^ (r >>> 7), 61 | r)) ^ r;
    return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
  };
}

const RESUME_ROWS = new Set(["continueWatching", "nextUp"]);
const MAX_PER_ROW = 2;

/**
 * Featured titles, drawn at random: unwatched films and series with a
 * backdrop, from the discovery rows (resume rows already have their shelf),
 * at most two per row so the lineup mixes libraries and servers. Falls back
 * to anything with a backdrop when the libraries are nearly all watched.
 */
function pickFeatured(rows: HomeRow[], seed: number): MediaItem[] {
  const rand = random(seed);
  const seen = new Set<string>();
  const pool: { item: MediaItem; row: number; key: number }[] = [];
  const fallback: { item: MediaItem; row: number; key: number }[] = [];
  rows.forEach((row, r) => {
    for (const item of row.items) {
      if (!item.images.backdrop || seen.has(item.id)) continue;
      seen.add(item.id);
      const entry = { item, row: r, key: rand() };
      const discovery = !RESUME_ROWS.has(row.kind.type);
      if (discovery && !item.user.played && (item.kind === "movie" || item.kind === "series")) pool.push(entry);
      else fallback.push(entry);
    }
  });
  const byKey = (a: { key: number }, b: { key: number }) => a.key - b.key;
  const draw = (candidates: typeof pool, out: MediaItem[], perRow: Map<number, number>) => {
    for (const c of candidates.sort(byKey)) {
      if (out.length >= MAX_SLIDES) break;
      const n = perRow.get(c.row) ?? 0;
      if (n >= MAX_PER_ROW) continue;
      perRow.set(c.row, n + 1);
      out.push(c.item);
    }
  };
  const out: MediaItem[] = [];
  const perRow = new Map<number, number>();
  draw(pool, out, perRow);
  // Too few after the per-row cap: lift it, then use the fallback.
  if (out.length < MAX_SLIDES) draw(pool.filter((c) => !out.includes(c.item)), out, new Map());
  if (out.length < MAX_SLIDES) draw(fallback, out, new Map());
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

  // Auto-advance is driven by the active dot's fill animation (below): it
  // pauses with hover/focus and resumes where it stopped.
  // Paused while looked at: pointer over it, or remote focus on it. Focus
  // alone does not count with a mouse: Home puts it on Play at launch.
  const modality = useModality((m) => m.modality);
  const paused = hover || (focusInside && modality === "keys");

  // The featured title sets the mood while it is being looked at.
  useEffect(() => {
    if (paused) ambientFor(item);
  }, [item, paused]);

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
                  className="relative h-2 w-2 cursor-pointer overflow-hidden rounded-full bg-white/30 transition-[width,background-color] duration-500 ease-apple data-[on=true]:w-8 data-[on=true]:bg-white/25"
                  data-on={i === index}
                >
                  {i === index && (
                    <span
                      key={index}
                      onAnimationEnd={() => go(1)}
                      className="absolute inset-0 origin-left rounded-full bg-white"
                      style={{ animation: `flick-progress ${SLIDE_MS}ms linear forwards`, animationPlayState: paused ? "paused" : "running" }}
                    />
                  )}
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
  const featured = useMemo(() => pickFeatured(rows, SESSION_SEED), [rows]);

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
          {errorText(home.error)}
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
