// A small notice over the video when someone writes in the room: the latest
// message only (a new one replaces it), gone after a few seconds. It stays out
// of the way: not while the chat itself is open, never for your own messages,
// and never for what was already in the chat when the player opened.
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import type { ChatMessage } from "@/ipc/bindings/ChatMessage";
import type { RoomState } from "@/ipc/bindings/RoomState";
import { enter } from "@/lib/motion";
import { useWatch } from "./store";

/** How long a message stays on screen. */
export const BANNER_MS = 5000;

/**
 * What to do when the room changed. `seen` is the id of the last message
 * already accounted for (`undefined`: nothing yet, so the chat as it is now is
 * history, not news). `show` is the message to put on screen, if any.
 */
export function bannerFor(room: RoomState | null, seen: number | null | undefined, chatOpen: boolean): { seen: number | null; show: ChatMessage | null } {
  const last = room?.chat[room.chat.length - 1];
  const id = last?.id ?? null;
  if (seen === undefined || id === seen) return { seen: id, show: null };
  const news = !!last && !!room?.chatEnabled && !chatOpen && last.sender_id !== room.you;
  return { seen: id, show: news ? last : null };
}

export function ChatBanner() {
  const room = useWatch((w) => w.room);
  const chatOpen = useWatch((w) => w.chatOpen);
  const [shown, setShown] = useState<ChatMessage | null>(null);
  const seen = useRef<number | null | undefined>(undefined);
  const lastId = room?.chat[room.chat.length - 1]?.id;

  useEffect(() => {
    const next = bannerFor(useWatch.getState().room, seen.current, useWatch.getState().chatOpen);
    seen.current = next.seen;
    if (next.show) setShown(next.show);
  }, [lastId]);

  // One message at a time, for a limited time (a new message restarts the clock).
  useEffect(() => {
    if (!shown) return;
    const t = window.setTimeout(() => setShown(null), BANNER_MS);
    return () => window.clearTimeout(t);
  }, [shown]);

  // Reading the chat, or chat turned off: nothing left to announce.
  const hidden = chatOpen || !room?.chatEnabled;
  useEffect(() => {
    if (hidden) setShown(null);
  }, [hidden]);

  return (
    <AnimatePresence>
      {shown && !hidden && (
        <motion.div
          key={shown.id}
          role="status"
          aria-live="polite"
          initial={{ opacity: 0, y: -10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -6 }}
          transition={enter}
          className="pointer-events-none absolute top-16 left-8 max-w-sm rounded-2xl bg-black/70 px-4 py-2.5 text-sm text-white shadow-[inset_0_0_0_1px_rgb(255_255_255/0.08),0_16px_40px_-16px_rgb(0_0_0/0.8)]"
        >
          <span className="font-semibold">{shown.sender_name || "Someone"}</span>
          {/* Plain text only: the server's text is never rendered as markup. */}
          <span className="line-clamp-2 break-words text-white/85">{shown.text}</span>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
