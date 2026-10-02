// The titles of one genre (movies or series, never mixed). Reached from a
// genre tile on the Search page: `/search/genre?kind=movie&name=Action`.
import { useInfiniteQuery } from "@tanstack/react-query";
import { useSearchParams } from "react-router";
import { BackButton } from "@/components/tv/BackButton";
import { Button } from "@/components/tv/Button";
import { MediaCard } from "@/components/tv/Card";
import { CenteredSpinner, Notice } from "@/components/tv/Feedback";
import { PageHeader } from "@/components/tv/Page";
import { api, asError } from "@/ipc/api";
import type { ItemKind } from "@/ipc/bindings/ItemKind";
import { FocusGroup, Screen } from "@/nav/Focusable";

/** Per server: with several servers a page holds up to this many from each. */
const PAGE = 60;

export function GenreGrid() {
  const [params] = useSearchParams();
  const kind: ItemKind = params.get("kind") === "series" ? "series" : "movie";
  const genre = params.get("name") ?? "";
  const noun = kind === "series" ? "TV shows" : "movies";

  const titles = useInfiniteQuery({
    queryKey: ["by-genre", kind, genre],
    initialPageParam: 0,
    queryFn: ({ pageParam }) => api.byGenre({ kind, genre, sort: "title", order: "ascending", start: pageParam, limit: PAGE }),
    getNextPageParam: (last, all) => (last.data.length >= PAGE ? all.length * PAGE : undefined),
  });
  const items = titles.data?.pages.flatMap((p) => p.data) ?? [];
  const issues = titles.data?.pages[0]?.issues ?? [];

  return (
    <Screen ready={!titles.isPending}>
      <div className="px-[var(--gutter)] pt-[var(--page-top)] -mb-[var(--page-top)]">
        <BackButton />
      </div>
      <div className="flex flex-col gap-8 px-[var(--gutter)] pt-[var(--page-top)] pb-24">
        <PageHeader title={genre} lead={titles.data ? `${items.length}${titles.hasNextPage ? "+" : ""} ${noun}` : undefined} />
        {titles.error && <Notice tone="error">{asError(titles.error).message}</Notice>}
        {issues.length > 0 && <Notice tone="warn">Some servers did not answer: {issues.map((i) => i.name).join(", ")}</Notice>}
        {titles.isPending ? (
          <CenteredSpinner />
        ) : items.length === 0 ? (
          <p className="text-lg text-muted-foreground">No {noun} in {genre} on your servers.</p>
        ) : (
          <>
            <FocusGroup focusKey="genre-results" className="grid grid-cols-[repeat(auto-fill,minmax(var(--poster-w),1fr))] gap-x-[var(--card-gap)] gap-y-8">
              {items.map((item) => (
                <MediaCard key={item.id} item={item} shape="poster" fluid focusKey={`genre-result:${item.id}`} />
              ))}
            </FocusGroup>
            {titles.hasNextPage && (
              <FocusGroup className="flex">
                <Button size="sm" disabled={titles.isFetchingNextPage} onClick={() => void titles.fetchNextPage()}>
                  {titles.isFetchingNextPage ? "Loading…" : "Load More"}
                </Button>
              </FocusGroup>
            )}
          </>
        )}
      </div>
    </Screen>
  );
}
