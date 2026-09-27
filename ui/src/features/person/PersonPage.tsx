// A person (actor, director…): TMDB biography, their titles on every
// server, and what else they are known for. Server data and TMDB arrive
// separately; each part shows as soon as it is ready.
import { useQuery } from "@tanstack/react-query";
import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router";
import { BackButton } from "@/components/tv/BackButton";
import { Button } from "@/components/tv/Button";
import { CardPlaceholder } from "@/components/tv/Card";
import { Notice } from "@/components/tv/Feedback";
import { Shelf } from "@/components/tv/Shelf";
import { api } from "@/ipc/api";
import type { PersonPhoto } from "@/ipc/bindings/PersonPhoto";
import { imageUrl, tmdbImageUrl } from "@/ipc/images";
import { ambientReset } from "@/lib/ambient";
import { enter } from "@/lib/motion";
import { alsoKnownFor, lifeLine, personShelves, uiLanguage } from "@/lib/person";
import { initials } from "@/lib/profiles";
import { useSettings } from "@/lib/settings";
import { FocusGroup, Screen } from "@/nav/Focusable";
import { KnownForCard } from "./KnownForCard";

/** A biography this long gets a More / Less toggle (the clamp shows ~5 lines). */
const LONG_BIOGRAPHY = 400;

function photoUrl(photo: PersonPhoto | null | undefined): string | undefined {
  if (!photo) return undefined;
  return photo.kind === "tmdb" ? tmdbImageUrl(photo.path, "h632") : imageUrl(photo.image, "large");
}

export function PersonPage() {
  const navigate = useNavigate();
  const { ref = "" } = useParams();
  const [params] = useSearchParams();
  const person = decodeURIComponent(ref);
  const name = params.get("name") ?? "";
  const from = params.get("from");
  const language = uiLanguage(useSettings()?.general.language ?? null, navigator.language);
  const details = useQuery({ queryKey: ["person", person, language], queryFn: () => api.personDetails(person, name, from, language), staleTime: Infinity });
  const items = useQuery({ queryKey: ["person-items", person], queryFn: () => api.personItems(person, name) });
  const [more, setMore] = useState(false);
  const [broken, setBroken] = useState(false);

  // The previous title's artwork must not linger behind someone else.
  useEffect(() => ambientReset(), [person]);

  const d = details.data;
  const onServers = items.data?.data ?? [];
  const known = d ? alsoKnownFor(d.knownFor, items.data?.data ?? (items.isError ? [] : undefined)) : [];
  const src = broken ? undefined : photoUrl(d?.photo);
  const line = d ? lifeLine(d, new Date()) : null;
  const shownName = d?.name ?? name;

  return (
    <Screen ready={!!d || !!items.data} className="pb-16">
      <header className="flex flex-col gap-6 px-[var(--gutter)] pt-[var(--page-top)] pb-8">
        <BackButton />
        <motion.div initial={{ y: 12 }} animate={{ y: 0 }} transition={enter} className="flex flex-wrap items-end gap-8">
          <div className="grid size-44 shrink-0 place-items-center overflow-hidden rounded-full bg-white/[0.08] shadow-[0_24px_60px_-24px_rgb(0_0_0/0.9)] ring-1 ring-white/10">
            {src ? (
              <img src={src} alt="" onError={() => setBroken(true)} className="size-full object-cover" />
            ) : (
              <span className="font-heading text-5xl font-bold text-white/60">{initials(shownName)}</span>
            )}
          </div>
          <div className="flex min-w-0 max-w-3xl flex-col gap-2">
            <h1 className="text-[2.75rem] leading-none font-bold tracking-tight">{shownName}</h1>
            {d?.department && <p className="text-[1.0625rem] text-white/60">{d.department}</p>}
            {line && <p className="text-[0.9375rem] text-white/70">{line}</p>}
          </div>
        </motion.div>
        {d?.biography && (
          <div className="flex max-w-3xl flex-col items-start gap-2">
            <p className={more ? "text-[1.0625rem] leading-relaxed whitespace-pre-line text-white/80" : "line-clamp-5 text-[1.0625rem] leading-relaxed text-white/80"}>
              {d.biography}
            </p>
            {d.biography.length > LONG_BIOGRAPHY && (
              <Button size="sm" variant="ghost" onClick={() => setMore(!more)}>
                {more ? "Less" : "More"}
              </Button>
            )}
          </div>
        )}
        {d?.tmdb === "noKey" && (
          <div className="flex max-w-3xl flex-wrap items-center gap-3">
            <p className="text-sm text-white/55">Add a TMDB key in Settings › Metadata for biographies.</p>
            <Button size="sm" variant="ghost" onClick={() => navigate("/settings?s=metadata")}>
              Open Settings
            </Button>
          </div>
        )}
        {items.data && items.data.issues.length > 0 && <Notice tone="warn">Some servers did not answer: {items.data.issues.map((i) => i.name).join(", ")}</Notice>}
      </header>

      {!items.data ? (
        <div className="flex gap-[var(--card-gap)] overflow-hidden px-[var(--gutter)] pt-6">
          {Array.from({ length: 6 }, (_, i) => (
            <CardPlaceholder key={i} shape="poster" />
          ))}
        </div>
      ) : (
        personShelves(onServers).map((s, i) => <Shelf key={s.title} id={`person-${s.title}`} index={i} title={s.title} items={s.items} shape="poster" />)
      )}

      {known.length > 0 && (
        <section className="relative">
          <h2 className="px-[var(--gutter)] text-[1.3125rem] font-semibold tracking-tight">Also known for</h2>
          <FocusGroup focusKey="shelf:known-for" fade="x" className="[--fade-size:var(--gutter)] no-scrollbar -mt-2 flex gap-[var(--card-gap)] overflow-x-auto px-[var(--gutter)] pt-6 pb-10">
            {known.map((c) => (
              <KnownForCard key={`${c.kind}:${c.tmdbId}`} credit={c} focusKey={`known-for:${c.kind}:${c.tmdbId}`} />
            ))}
          </FocusGroup>
        </section>
      )}
    </Screen>
  );
}
