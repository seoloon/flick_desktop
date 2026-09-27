// Favourites of the active profile: the query and how they split into
// shelves (fixed order, kinds without a shelf of their own go to "Other").
import { api } from "@/ipc/api";
import type { ItemKind } from "@/ipc/bindings/ItemKind";
import type { MediaItem } from "@/ipc/bindings/MediaItem";

export const favoritesQuery = { queryKey: ["favorites"], queryFn: () => api.favorites() };

type Shape = "poster" | "thumb";

const GROUPS: { kinds: ItemKind[]; title: string; shape: Shape }[] = [
  { kinds: ["movie"], title: "Movies", shape: "poster" },
  { kinds: ["series"], title: "TV Shows", shape: "poster" },
  { kinds: ["episode"], title: "Episodes", shape: "thumb" },
  { kinds: ["collection"], title: "Collections", shape: "poster" },
];

export function favoriteShelves(items: MediaItem[]): { title: string; shape: Shape; items: MediaItem[] }[] {
  const known = new Set(GROUPS.flatMap((g) => g.kinds));
  const shelves = GROUPS.map((g) => ({ title: g.title, shape: g.shape, items: items.filter((i) => g.kinds.includes(i.kind)) }));
  shelves.push({ title: "Other", shape: "poster", items: items.filter((i) => !known.has(i.kind)) });
  return shelves.filter((s) => s.items.length > 0);
}
