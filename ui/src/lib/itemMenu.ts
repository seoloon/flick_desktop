// State behind the right-click menu on media: which item it is open for and
// where, plus per-item "watched / favourite" overrides so every card showing
// that item updates at once, whichever list it came from.
import { useMemo } from "react";
import { create } from "zustand";
import { api } from "@/ipc/api";
import type { ItemRef } from "@/ipc/bindings/ItemRef";
import type { MediaItem } from "@/ipc/bindings/MediaItem";
import type { UserState } from "@/ipc/bindings/UserState";
import { queryClient } from "./queryClient";

type Override = Partial<Pick<UserState, "played" | "positionMs" | "favorite">>;

type ItemMenuState = {
  target: { item: MediaItem; x: number; y: number } | null;
  overrides: Record<ItemRef, Override>;
};

export const useItemMenu = create<ItemMenuState>(() => ({ target: null, overrides: {} }));

export function openItemMenu(item: MediaItem, x: number, y: number) {
  useItemMenu.setState({ target: { item, x, y } });
}

export function closeItemMenu() {
  if (useItemMenu.getState().target) useItemMenu.setState({ target: null });
}

/** A profile switch clears every cache; the overrides go with it. */
export function resetItemOverrides() {
  useItemMenu.setState({ target: null, overrides: {} });
}

/** The item as the user last left it, even before a list refetches. */
export function useOverridden<T extends MediaItem | undefined>(item: T): T {
  const override = useItemMenu((s) => (item ? s.overrides[item.id] : undefined));
  return useMemo(() => (item && override ? { ...item, user: { ...item.user, ...override } } : item), [item, override]);
}

/** Toggles watched or favourite on the server, then everywhere it shows. Throws on failure. */
export async function setItemFlag(item: MediaItem, what: "played" | "favorite") {
  const patch: Override =
    what === "played" ? { played: !item.user.played, positionMs: 0 } : { favorite: !item.user.favorite };
  if (what === "played") await api.setPlayed(item.id, patch.played!);
  else await api.setFavorite(item.id, patch.favorite!);

  useItemMenu.setState((s) => ({ overrides: { ...s.overrides, [item.id]: { ...s.overrides[item.id], ...patch } } }));
  queryClient.setQueryData<MediaItem>(["item", item.id], (old) => (old ? { ...old, user: { ...old.user, ...patch } } : old));
  void queryClient.invalidateQueries({ queryKey: ["home"] });
  void queryClient.invalidateQueries({ queryKey: ["favorites"] });
}
