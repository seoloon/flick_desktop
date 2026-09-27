import { describe, expect, it, vi } from "vitest";
import type { KnownFor } from "@/ipc/bindings/KnownFor";
import type { MediaItem } from "@/ipc/bindings/MediaItem";

vi.mock("@/ipc/api", () => ({ api: {} }));

import { age, alsoKnownFor, lifeLine, personPath, personShelves, uiLanguage } from "./person";

const kf = (tmdbId: string, title: string, year: number, votes: number): KnownFor =>
  ({ kind: "movie", title, year, role: null, poster: null, tmdbId, voteCount: votes }) as KnownFor;
const item = (id: string, kind: MediaItem["kind"], tmdb: string | null, title = id, year: number | null = null) =>
  ({ id, kind, title, year, externalIds: { imdb: null, tmdb, tvdb: null } }) as unknown as MediaItem;

describe("person helpers", () => {
  it("builds the page path", () => {
    expect(personPath({ person: "s:p1", name: "Tom Hanks" }, "s:m1")).toBe("/person/s%3Ap1?name=Tom%20Hanks&from=s%3Am1");
    expect(personPath({ person: "s:p1", name: "Tom Hanks" })).toBe("/person/s%3Ap1?name=Tom%20Hanks");
  });

  it("computes ages", () => {
    expect(age("1956-07-09", new Date("2026-07-08"))).toBe(69);
    expect(age("1956-07-09", new Date("2026-07-09"))).toBe(70);
  });

  it("writes the life line", () => {
    const base = { name: "", department: null, biography: null, photo: null, knownFor: [], tmdb: "used" } as const;
    expect(lifeLine({ ...base, birth: "1956-07-09", death: null, birthplace: "Concord" }, new Date("2026-09-28"))).toBe("Born 9 July 1956 (age 70) · Concord");
    expect(lifeLine({ ...base, birth: "1932-04-10", death: "2016-12-28", birthplace: null }, new Date("2026-09-28"))).toBe("1932 – 2016 (aged 84)");
    expect(lifeLine({ ...base, birth: null, death: null, birthplace: null }, new Date())).toBeNull();
  });

  it("keeps known-for titles that are not on the servers, most voted first, 20 at most", () => {
    const credits = [kf("13", "Forrest Gump", 1994, 900), kf("862", "Toy Story", 1995, 800), kf("7", "Big", 1988, 100)];
    const onServers = [item("a", "movie", "13"), item("b", "movie", null, "Big", 1988)];
    expect(alsoKnownFor(credits, onServers).map((c) => c.title)).toEqual(["Toy Story"]);
    expect(alsoKnownFor(Array.from({ length: 30 }, (_, i) => kf(String(i), `T${i}`, 2000, 30 - i)), [])).toHaveLength(20);
  });

  it("waits for the servers' titles before saying what they lack", () => {
    expect(alsoKnownFor([kf("13", "Forrest Gump", 1994, 900)], undefined)).toEqual([]);
  });

  it("splits the servers' titles into shelves", () => {
    const shelves = personShelves([item("s", "series", null), item("m", "movie", null)]);
    expect(shelves.map((s) => [s.title, s.items.map((i) => i.id)])).toEqual([["Movies", ["m"]], ["TV Shows", ["s"]]]);
  });

  it("picks the TMDB language", () => {
    expect(uiLanguage("fr", "en-US")).toBe("fr");
    expect(uiLanguage(null, "fr-FR")).toBe("fr-FR");
  });
});
