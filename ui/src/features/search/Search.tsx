import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { MediaCard } from "@/components/tv/Card";
import { Notice, Spinner } from "@/components/tv/Feedback";
import { PageHeader } from "@/components/tv/Page";
import { Shelf } from "@/components/tv/Shelf";
import { TextField } from "@/components/tv/TextField";
import { api } from "@/ipc/api";
import type { ItemKind } from "@/ipc/bindings/ItemKind";
import { FocusGroup, Screen } from "@/nav/Focusable";
import { GenreTile } from "./GenreTile";

export function Search() {
  const [term, setTerm] = useState("");
  const [query, setQuery] = useState("");
  useEffect(() => {
    const t = window.setTimeout(() => setQuery(term.trim()), 280);
    return () => window.clearTimeout(t);
  }, [term]);
  const searching = query.length >= 2;
  const results = useQuery({ queryKey: ["search", query], queryFn: () => api.search(query), enabled: searching });

  return (
    <Screen>
      <div className="flex flex-col gap-8 pt-[var(--page-top)] pb-24">
        <div className="flex flex-col gap-8 px-[var(--gutter)]">
          <PageHeader title="Search" />
          <div className="max-w-2xl">
            <TextField label="Titles, people, collections" large value={term} onChange={setTerm} autoFocus placeholder="Titles, people, collections" />
          </div>
          {results.isFetching && <Spinner />}
          {results.data && (
            <>
              {results.data.issues.length > 0 && <Notice tone="warn">Some servers did not answer: {results.data.issues.map((i) => i.name).join(", ")}</Notice>}
              {results.data.data.length === 0 ? (
                <p className="text-lg text-muted-foreground">Nothing matches “{query}” on your servers.</p>
              ) : (
                <FocusGroup focusKey="results" className="grid grid-cols-[repeat(auto-fill,minmax(var(--poster-w),1fr))] gap-x-[var(--card-gap)] gap-y-8">
                  {results.data.data.map((item) => (
                    <MediaCard key={item.id} item={item} shape="poster" fluid focusKey={`result:${item.id}`} />
                  ))}
                </FocusGroup>
              )}
            </>
          )}
        </div>
        {!searching && <Discover />}
      </div>
    </Screen>
  );
}

/** Before anything is typed: what to watch next, then the genres to browse. */
function Discover() {
  const recommendations = useQuery({ queryKey: ["recommendations"], queryFn: () => api.recommendations(), staleTime: 5 * 60_000 });
  return (
    <>
      {/* Nothing watched yet means no rows: the section is simply absent. */}
      {recommendations.data?.data.map((row, i) => (
        <Shelf key={row.title} id={`recommended:${i}`} index={i} title={row.title} items={row.items} shape="poster" />
      ))}
      <Genres kind="movie" title="Movie Genres" />
      <Genres kind="series" title="TV Show Genres" />
    </>
  );
}

function Genres({ kind, title }: { kind: ItemKind; title: string }) {
  const genres = useQuery({ queryKey: ["genres", kind], queryFn: () => api.genres(kind), staleTime: 10 * 60_000 });
  const names = genres.data?.data ?? [];
  if (names.length === 0) return null;
  return (
    <section className="flex flex-col gap-4 px-[var(--gutter)]">
      <h2 className="text-[1.3125rem] font-semibold tracking-tight text-white">{title}</h2>
      {/* Room for the focused tile to grow without being clipped. */}
      <FocusGroup focusKey={`genres:${kind}`} className="grid grid-cols-[repeat(auto-fill,minmax(14rem,1fr))] gap-[var(--card-gap)] py-3">
        {names.map((name) => (
          <GenreTile key={name} kind={kind} genre={name} />
        ))}
      </FocusGroup>
    </section>
  );
}
