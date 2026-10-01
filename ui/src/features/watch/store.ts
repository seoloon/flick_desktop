// The one client-side copy of the room: whatever Rust last said. Components
// read it from here and never keep their own.
import { create } from "zustand";
import { api } from "@/ipc/api";
import type { FlickSyncStatus } from "@/ipc/app-types";
import type { RoomState } from "@/ipc/bindings/RoomState";
import type { UserMessage } from "@/ipc/bindings/UserMessage";

type WatchState = {
  room: RoomState | null;
  status: FlickSyncStatus | null;
  /** Chat messages received while the chat was not in view. */
  unread: number;
  chatOpen: boolean;
  setRoom: (room: RoomState | null) => void;
  setStatus: (status: FlickSyncStatus | null) => void;
  setChatOpen: (open: boolean) => void;
};

export const useWatch = create<WatchState>((set, get) => ({
  room: null,
  status: null,
  unread: 0,
  chatOpen: false,
  setRoom: (room) => {
    const prev = get().room;
    const grew = room && prev && room.roomId === prev.roomId ? Math.max(0, room.chat.length - prev.chat.length) : 0;
    set({ room, unread: room ? (get().chatOpen ? 0 : get().unread + grew) : 0 });
  },
  setStatus: (status) => set({ status }),
  setChatOpen: (open) => set({ chatOpen: open, unread: open ? 0 : get().unread }),
}));

export const inRoom = () => useWatch.getState().room !== null;

export async function refreshStatus() {
  try {
    useWatch.getState().setStatus(await api.flicksyncStatus());
  } catch {
    // Never leave the screen waiting for an answer that is not coming.
    useWatch.getState().setStatus({ available: false, configured: false, inRoom: false, message: "unavailable" });
  }
}

/** What to tell the user, by code (English fallbacks; mirrors the Rust ones). */
const MESSAGES: Record<UserMessage, string> = {
  only_host_can_choose_media: "Only the host can choose what the room watches.",
  control_denied: "Only the host can control playback in this room.",
  room_not_found: "This room doesn't exist anymore.",
  room_full: "This room is full.",
  room_closed: "The room was closed.",
  invalid_media: "The host picked something Flick can't share.",
  session_expired: "Your session expired. Please try again.",
  slow_down: "You're doing that too fast.",
  chat_disabled: "Chat is turned off in this room.",
  incompatible_version: "Your Flick version is not compatible with this FlickSync server.",
  unavailable: "Watch together isn't reachable right now.",
  media_unavailable: "This item isn't available on your connected server.",
  not_configured: "Watch together isn't set up.",
  generic: "Something went wrong with the watch room.",
};

export const messageText = (m: UserMessage) => MESSAGES[m] ?? MESSAGES.generic;

/** Why a session ended, for the toast. `null`: nothing to say (the user left). */
export function leftText(reason: string | null): string | null {
  switch (reason) {
    case null:
      return null;
    case "host_closed":
    case "host_left":
    case "closed":
      return "The room was closed.";
    case "expired":
    case "empty":
      return "The room expired.";
    case "removed":
      return "You were removed from the room.";
    case "replaced":
      return "You joined this room from another device.";
    case "unavailable":
      return "Lost the connection to the room. You can rejoin it.";
    default:
      return null;
  }
}
