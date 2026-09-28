// Virtualized library grid: only visible rows exist in the DOM, and pages of
// items are fetched on demand as rows scroll into view, so a 20 000-title
// library costs the same as a 50-title one. The grid scrolls with the screen
// (the shell's <main>), so the header scrolls away like on tvOS.
import { useVirtualizer } from "@tanstack/react-virtual";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useParams, useSearchParams } from "react-router";
import { CardPlaceholder, MediaCard } from "@/components/tv/Card";
import { CenteredSpinner, Notice } from "@/components/tv/Feedback";
import { PageHeader } from "@/components/tv/Page";
import { Segmented } from "@/components/tv/Segmented";
import { BackButton } from "@/components/tv/BackButton";
import { api, asError } from "@/ipc/api";
import type { ItemKind } from "@/ipc/bindings/ItemKind";
import type { MediaItem } from "@/ipc/bindings/MediaItem";
import type { SortBy } from "@/ipc/bindings/SortBy";
import { useMode } from "@/lib/mode";
import { useSettings } from "@/lib/settings";
import { FocusGroup, Screen } from "@/nav/Focusable";

const PAGE = 120;

type Metrics = { rem: number; gap: number; poster: number };

/**
 * Root font size and the card tokens, in pixels. Read once per grid width,
 * not per render: the virtualizer re-renders on every scroll frame, and
 * `getComputedStyle` there would force a style recalculation each time.
 */
function readMetrics(): Metrics {
  const style = getComputedStyle(document.documentElement);
  const rem = parseFloat(style.fontSize) || 16;
  const token = (name: string, fallback: number) => (parseFloat(style.getPropertyValue(name)) || fallback) * rem;
  return { rem, gap: token("--card-gap", 1.5), poster: token("--poster-w", 10.5) };
}

export function LibraryGrid() {
  const { id = "" } = useParams();
  const [search] = useSearchParams();
  const parent = decodeURIComponent(id);
  const kind = search.get("kind");
  const kinds: ItemKind[] = kind === "movies" ? ["movie"] : kind === "shows" ? ["series"] : [];

  const [sort, setSort] = useState<SortBy>("title");
  const [filter, setFilter] = useState<"all" | "unplayed">("all");
  const [items, setItems] = useState<Record<number, MediaItem>>({});
  const [total, setTotal] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const loaded = useRef(new Set<number>());
  const generation = useRef(0);

  // Layout: columns from the grid width and the poster token.
  const grid = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(1200);
  const [scrollMargin, setScrollMargin] = useState(0);
  // The tokens follow the density and Flick Frame (and the root font size, the window width).
  const density = useSettings()?.appearance.density;
  const frame = useMode((s) => s.frame);
  const metrics = useMemo(() => readMetrics(), [width, density, frame]); // eslint-disable-line react-hooks/exhaustive-deps
  const gap = metrics.gap;
  const columns = Math.max(2, Math.floor((width + gap) / (metrics.poster + gap)));
  const cellW = (width - gap * (columns - 1)) / columns;
  const rowHeight = cellW * 1.5 + metrics.rem * 3.25 + gap;
  const rowCount = Math.ceil((total ?? 0) / columns);

  useLayoutEffect(() => {
    const el = grid.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => {
      if (e) setWidth(e.contentRect.width);
      setScrollMargin(el.offsetTop);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [total !== null]);

  const loadPage = useCallback(
    async (page: number) => {
      if (loaded.current.has(page)) return;
      loaded.current.add(page);
      const gen = generation.current;
      try {
        const res = await api.items({
          parent,
          kinds,
          filter: { genres: [], years: [], person: null, unplayedOnly: filter === "unplayed", favoritesOnly: false },
          sort,
          order: sort === "title" ? "ascending" : "descending",
          start: page * PAGE,
          limit: PAGE,
        });
        if (gen !== generation.current) return;
        setTotal(res.total ?? res.start + res.items.length);
        setItems((prev) => {
          const next = { ...prev };
          res.items.forEach((it, i) => (next[res.start + i] = it));
          return next;
        });
      } catch (e) {
        loaded.current.delete(page);
        setError(asError(e).message);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [parent, sort, filter, kind],
  );

  // New query: start over from the first page.
  useEffect(() => {
    generation.current++;
    loaded.current.clear();
    setItems({});
    setTotal(null);
    setError(null);
    void loadPage(0);
  }, [loadPage]);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => grid.current?.closest("main") ?? null,
    estimateSize: () => rowHeight,
    overscan: 3,
    scrollMargin,
  });
  useEffect(() => virtualizer.measure(), [rowHeight, virtualizer]);

  const rows = virtualizer.getVirtualItems();
  // Fetch whichever pages intersect the rendered rows.
  useEffect(() => {
    for (const row of rows) {
      const first = row.index * columns;
      void loadPage(Math.floor(first / PAGE));
      void loadPage(Math.floor(Math.min(first + columns - 1, (total ?? 1) - 1) / PAGE));
    }
  }, [rows, columns, total, loadPage]);

  return (
    <Screen ready={total !== null}>
      <div className="px-[var(--gutter)] pt-[var(--page-top)] -mb-[var(--page-top)]">
        <BackButton />
      </div>
      <div className="flex flex-col gap-8 px-[var(--gutter)] pt-[var(--page-top)] pb-24">
        <PageHeader
          title={search.get("name") ?? "Library"}
          lead={total !== null ? `${total} titles` : undefined}
          actions={
            <>
              <Segmented
                label="Sort"
                options={[
                  { value: "title", label: "A–Z" },
                  { value: "dateAdded", label: "Recently Added" },
                  { value: "releaseDate", label: "Release Date" },
                  { value: "rating", label: "Rating" },
                ]}
                value={sort}
                onChange={(v) => setSort(v as SortBy)}
              />
              <Segmented
                label="Filter"
                options={[
                  { value: "all", label: "All" },
                  { value: "unplayed", label: "Unwatched" },
                ]}
                value={filter}
                onChange={setFilter}
              />
            </>
          }
        />
        {error && <Notice tone="error">{error}</Notice>}
        {total === null ? (
          <CenteredSpinner />
        ) : (
          <div ref={grid}>
            <FocusGroup focusKey="grid" className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
              {rows.map((row) => (
                <div
                  key={row.key}
                  className="absolute inset-x-0 top-0 grid"
                  style={{
                    transform: `translateY(${row.start - scrollMargin}px)`,
                    gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
                    columnGap: gap,
                  }}
                >
                  {Array.from({ length: columns }, (_, c) => row.index * columns + c)
                    .filter((i) => i < (total ?? 0))
                    .map((i) => {
                      const it = items[i];
                      return it ? <MediaCard key={i} item={it} shape="poster" fluid focusKey={`grid:${i}`} /> : <CardPlaceholder key={i} shape="poster" fluid />;
                    })}
                </div>
              ))}
            </FocusGroup>
          </div>
        )}
      </div>
    </Screen>
  );
}
