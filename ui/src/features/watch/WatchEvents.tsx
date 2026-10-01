// Listens to the room for the whole app lifetime (not just while the Watch
// screen is open: leaving the screen must not leave the room).
import { useEffect, useRef } from "react";
import { useLocation, useNavigate } from "react-router";
import { toast } from "sonner";
import { api } from "@/ipc/api";
import type { ItemRef } from "@/ipc/bindings/ItemRef";
import { onFlickSyncEvent } from "@/ipc/events";
import { flushSettings, useSettings } from "@/lib/settings";
import { playPath } from "@/features/player/route";
import { leftText, messageText, refreshStatus, useWatch } from "./store";

export function WatchEvents() {
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const onPlayer = useRef(false);
  onPlayer.current = pathname === "/play";
  const flicksync = useSettings()?.flicksync;

  // Availability follows the settings (server address, key, switch).
  // Settings reach Rust after a short debounce: push them first, then ask.
  useEffect(() => {
    void flushSettings().then(refreshStatus, refreshStatus);
  }, [flicksync?.enabled, flicksync?.flickServerUrl, flicksync?.syncUrl]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let alive = true;
    const store = useWatch.getState();
    // A reload of the window must not forget a room that is still open.
    void api.flicksyncState().then((room) => alive && room && store.setRoom(room), () => undefined);
    void onFlickSyncEvent((e) => {
      switch (e.type) {
        case "state":
          useWatch.getState().setRoom(e.room);
          break;
        case "notice":
          toast(messageText(e.message));
          break;
        case "mediaUnavailable":
          toast.error(e.title ? `“${e.title}” isn't available on your connected server.` : messageText("media_unavailable"), {
            description: "You're still in the room.",
          });
          break;
        case "left": {
          useWatch.getState().setRoom(null);
          const text = leftText(e.reason);
          if (text) toast(text);
          break;
        }
        case "openPlayer":
          // Same screen, new title: replace instead of piling up history.
          navigate(playPath(e.item as ItemRef), { replace: onPlayer.current });
          break;
      }
    }).then((u) => (alive ? (unlisten = u) : u()));
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [navigate]);

  return null;
}
