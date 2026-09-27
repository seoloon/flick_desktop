import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { MediaCard } from "@/components/tv/Card";
import { Notice, Spinner } from "@/components/tv/Feedback";
import { Page, PageHeader } from "@/components/tv/Page";
import { TextField } from "@/components/tv/TextField";
import { api } from "@/ipc/api";
import { FocusGroup, Screen } from "@/nav/Focusable";

export function Search() {
  const [term, setTerm] = useState("");
  const [query, setQuery] = useState("");
  useEffect(() => {
    const t = window.setTimeout(() => setQuery(term.trim()), 280);
    return () => window.clearTimeout(t);
  }, [term]);
  const results = useQuery({ queryKey: ["search", query], queryFn: () => api.search(query), enabled: query.length >= 2 });

  return (
    <Screen>
      <Page>
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
      </Page>
    </Screen>
  );
}
