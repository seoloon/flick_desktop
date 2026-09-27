import type { ItemRef } from "@/ipc/bindings/ItemRef";
import type { MediaItem } from "@/ipc/bindings/MediaItem";

export function playPath(id: ItemRef, startMs = 0, extra: Record<string, string> = {}) {
  const q = new URLSearchParams({ item: id, start: String(Math.floor(startMs)), ...extra });
  return `/play?${q.toString()}`;
}

/** Containers open their detail page; everything else plays. */
export function isPlayable(item: MediaItem): boolean {
  return !["series", "season", "collection", "playlist", "folder", "person"].includes(item.kind);
}
