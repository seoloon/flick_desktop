// Offline mode: asks Rust whether any server answers, at launch, when the
// network changes, and every so often. The catalogue follows (downloads only
// while offline); screens refetch when the mode flips.
import { useEffect } from "react";
import { create } from "zustand";
import { api } from "@/ipc/api";
import { queryClient } from "@/lib/queryClient";

type OfflineState = { offline: boolean; setOffline: (v: boolean) => void };
export const useOffline = create<OfflineState>((set) => ({ offline: false, setOffline: (offline) => set({ offline }) }));

const ONLINE_EVERY = 60_000;
const OFFLINE_EVERY = 15_000;

export async function checkOffline() {
  try {
    const offline = await api.offlineCheck();
    if (offline !== useOffline.getState().offline) {
      useOffline.getState().setOffline(offline);
      // What was fetched from the servers (or from the downloads) is no longer what to show.
      await queryClient.invalidateQueries();
    }
  } catch {
    // The check itself failing says nothing about the servers: keep the mode.
  }
}

export function OfflineWatcher() {
  const offline = useOffline((s) => s.offline);
  useEffect(() => {
    void checkOffline();
    const on = () => void checkOffline();
    window.addEventListener("online", on);
    window.addEventListener("offline", on);
    return () => {
      window.removeEventListener("online", on);
      window.removeEventListener("offline", on);
    };
  }, []);
  useEffect(() => {
    const t = setInterval(() => void checkOffline(), offline ? OFFLINE_EVERY : ONLINE_EVERY);
    return () => clearInterval(t);
  }, [offline]);
  return null;
}
