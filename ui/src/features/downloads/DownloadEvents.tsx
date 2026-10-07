// Listens to the downloads for the whole app lifetime, so progress is current
// whichever screen is open.
import { useEffect } from "react";
import { toast } from "sonner";
import { onDownloadsEvent } from "@/ipc/events";
import { refreshDownloads, useDownloads } from "./store";

export function DownloadEvents() {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let alive = true;
    void onDownloadsEvent((e) => {
      const before = e.type === "changed" ? useDownloads.getState().items[e.item.id] : undefined;
      useDownloads.getState().apply(e);
      if (e.type === "changed" && before && before.state !== e.item.state) {
        if (e.item.state === "done") toast.success(`Downloaded: ${e.item.title}`, { description: e.item.subtitle ?? undefined });
        else if (e.item.state === "failed") toast.error(`Download failed: ${e.item.title}`, { description: e.item.error ?? undefined });
      }
    }).then((u) => (alive ? (unlisten = u) : u()));
    void refreshDownloads();
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
  return null;
}
