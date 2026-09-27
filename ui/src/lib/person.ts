// Person pages: paths, dates and how the page splits what the servers have
// from what TMDB knows (pure helpers, tested).
import type { ItemRef } from "@/ipc/bindings/ItemRef";
import type { KnownFor } from "@/ipc/bindings/KnownFor";
import type { MediaItem } from "@/ipc/bindings/MediaItem";
import type { PersonDetails } from "@/ipc/bindings/PersonDetails";

export function personPath(credit: { person: ItemRef; name: string }, from?: ItemRef): string {
  const base = `/person/${encodeURIComponent(credit.person)}?name=${encodeURIComponent(credit.name)}`;
  return from ? `${base}&from=${encodeURIComponent(from)}` : base;
}

function utc(date: string): Date {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(Date.UTC(y!, (m ?? 1) - 1, d ?? 1));
}

/** Whole years from `birth` (YYYY-MM-DD) to `until`. */
export function age(birth: string, until: Date): number {
  const b = utc(birth);
  let years = until.getUTCFullYear() - b.getUTCFullYear();
  const before = until.getUTCMonth() < b.getUTCMonth() || (until.getUTCMonth() === b.getUTCMonth() && until.getUTCDate() < b.getUTCDate());
  if (before) years -= 1;
  return years;
}

const longDate = (date: string) => utc(date).toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" });

/** "Born 9 July 1956 (age 70) · Concord", "1932 – 2016 (aged 84)", or null. */
export function lifeLine(d: Pick<PersonDetails, "birth" | "death" | "birthplace">, today: Date): string | null {
  const parts: string[] = [];
  if (d.birth && d.death) parts.push(`${d.birth.slice(0, 4)} – ${d.death.slice(0, 4)} (aged ${age(d.birth, utc(d.death))})`);
  else if (d.birth) parts.push(`Born ${longDate(d.birth)} (age ${age(d.birth, today)})`);
  else if (d.death) parts.push(`Died ${longDate(d.death)}`);
  if (d.birthplace) parts.push(d.birthplace);
  return parts.length ? parts.join(" · ") : null;
}

const norm = (s: string) => s.normalize("NFKD").replace(/\p{M}/gu, "").toLowerCase().replace(/\s+/g, " ").trim();

/**
 * TMDB titles the servers do not have (same kind and TMDB id, or same title and
 * year), most voted first. Empty until the servers' titles are known, so
 * cards never vanish under the user as they load.
 */
export function alsoKnownFor(credits: KnownFor[], onServers: MediaItem[] | undefined, limit = 20): KnownFor[] {
  if (!onServers) return [];
  // TMDB numbers movies and series separately: the kind is part of the id.
  const ids = new Set(onServers.flatMap((i) => (i.externalIds.tmdb ? [`${i.kind}:${i.externalIds.tmdb}`] : [])));
  const titles = new Set(onServers.map((i) => `${norm(i.title)}|${i.year ?? ""}`));
  return credits
    .filter((c) => !ids.has(`${c.kind}:${c.tmdbId}`) && !titles.has(`${norm(c.title)}|${c.year ?? ""}`))
    .sort((a, b) => b.voteCount - a.voteCount)
    .slice(0, limit);
}

export function personShelves(items: MediaItem[]): { title: string; items: MediaItem[] }[] {
  return [
    { title: "Movies", items: items.filter((i) => i.kind === "movie") },
    { title: "TV Shows", items: items.filter((i) => i.kind === "series") },
  ].filter((s) => s.items.length > 0);
}

/** The language TMDB answers in: the app's setting, else the system's. */
export function uiLanguage(settingsLanguage: string | null, navigatorLanguage: string): string {
  return settingsLanguage || navigatorLanguage;
}
