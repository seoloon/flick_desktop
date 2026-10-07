// The one client-side copy of the downloads: whatever Rust last said.
import { create } from "zustand";
import { api } from "@/ipc/api";
import type { DownloadEvent, DownloadItem, DownloadsStatus } from "@/ipc/app-types";

type DownloadsState = {
  items: Record<string, DownloadItem>;
  status: DownloadsStatus | null;
  apply: (e: DownloadEvent) => void;
  setAll: (items: DownloadItem[]) => void;
  setStatus: (s: DownloadsStatus | null) => void;
};

export const useDownloads = create<DownloadsState>((set) => ({
  items: {},
  status: null,
  apply: (e) =>
    set((s) => {
      if (e.type === "changed") return { items: { ...s.items, [e.item.id]: e.item } };
      const { [e.id]: _gone, ...rest } = s.items;
      return { items: rest };
    }),
  setAll: (items) => set({ items: Object.fromEntries(items.map((d) => [d.id, d])) }),
  setStatus: (status) => set({ status }),
}));

export async function refreshDownloads() {
  try {
    useDownloads.getState().setAll(await api.downloadsList());
    useDownloads.getState().setStatus(await api.downloadsStatus());
  } catch {
    // Downloads are optional: a failure here leaves the list as it was.
  }
}

/** The download of a library item, if any (a failed one does not count). */
export function downloadOf(items: Record<string, DownloadItem>, itemRef: string): DownloadItem | undefined {
  return Object.values(items).find((d) => d.itemRef === itemRef && d.state !== "failed");
}
