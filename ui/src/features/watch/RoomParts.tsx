// Pieces of the room shared by the Watch screen and the player's room panel:
// participants, host controls and chat. They only read the room (from Rust)
// and send actions; none keeps its own copy of the room.
import { Send } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { Pill } from "@/components/tv/Page";
import { ToggleRow } from "@/components/tv/SettingsList";
import { TextField } from "@/components/tv/TextField";
import { api } from "@/ipc/api";
import type { Participant } from "@/ipc/bindings/Participant";
import type { RoomState } from "@/ipc/bindings/RoomState";
import { cn } from "@/lib/utils";
import { FocusGroup } from "@/nav/Focusable";
import { useWatch } from "./store";
import { errorText } from "@/lib/errors";

export const initials = (name: string) =>
  name
    .trim()
    .split(/\s+/)
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase() ?? "")
    .join("") || "?";

/** What a participant is doing, derived from what the room and the connection say. */
export function participantState(p: Participant, room: RoomState): string {
  if (p.presence !== "connected") return "Reconnecting…";
  if (!room.media) return "Waiting";
  return room.playback?.state === "playing" ? "Watching" : "Paused";
}

export const isHost = (room: RoomState) => room.hostId === room.you;

/** Shows a failed action as a toast instead of an unhandled rejection. */
export const act = (p: Promise<unknown>) => p.catch((e) => toast.error(errorText(e)));

const timeFormat = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });

/** Never throws: a bad timestamp from the server must not take the screen down. */
function formatTime(ms: number): string {
  try {
    const d = new Date(ms);
    return Number.isFinite(d.getTime()) ? timeFormat.format(d) : "";
  } catch {
    return "";
  }
}

export function ParticipantsList({ room }: { room: RoomState }) {
  return (
    <ul className="flex flex-col gap-3">
      {room.participants.map((p) => (
        <li key={p.participant_id} className={cn("flex items-center gap-3", p.presence !== "connected" && "opacity-60")}>
          <span className="grid size-10 shrink-0 place-items-center rounded-full bg-white/15 text-sm font-bold">{initials(p.display_name)}</span>
          <span className="min-w-0 flex-1 truncate font-medium">
            {p.display_name}
            {p.participant_id === room.you && <span className="text-muted-foreground"> (you)</span>}
          </span>
          {p.is_host && <Pill tone="strong">Host</Pill>}
          <span className="w-24 shrink-0 text-right text-sm text-muted-foreground">{participantState(p, room)}</span>
        </li>
      ))}
    </ul>
  );
}

/** Host-only room settings. Renders nothing for guests (the server enforces it anyway). */
export function HostControls({ room }: { room: RoomState }) {
  if (!isHost(room)) return null;
  return (
    <>
      <ToggleRow
        label="Everyone can control playback"
        hint="When off, only you can play, pause and seek."
        checked={room.controlMode === "everyone"}
        onChange={(v) => void act(api.flicksyncUpdateRoom(!v, null))}
      />
      <ToggleRow label="Chat" checked={room.chatEnabled} onChange={(v) => void act(api.flicksyncUpdateRoom(null, v))} />
      <FocusGroup className="flex">
        <Button variant="danger" size="sm" onClick={() => void act(api.flicksyncCloseRoom())}>
          Close Room for Everyone
        </Button>
      </FocusGroup>
    </>
  );
}

export function ChatView({ room, className }: { room: RoomState; className?: string }) {
  const [text, setText] = useState("");
  const box = useRef<HTMLDivElement>(null);

  // Reading the chat: nothing is unread while it is on screen.
  useEffect(() => {
    useWatch.getState().setChatOpen(true);
    return () => useWatch.getState().setChatOpen(false);
  }, []);
  // Scroll the message list only. `scrollIntoView` also scrolls every ancestor
  // and shifts the whole screen.
  useEffect(() => {
    const el = box.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [room.chat.length]);

  const send = () => {
    const t = text.trim();
    if (!t) return;
    setText("");
    act(api.flicksyncChat(t));
  };

  if (!room.chatEnabled) return <p className="text-sm text-muted-foreground">Chat is turned off in this room.</p>;

  return (
    <div className={cn("flex flex-col gap-3", className)}>
      <div ref={box} className="flex max-h-72 min-h-24 flex-col gap-2 overflow-y-auto pr-1 text-sm">
        {room.chat.length === 0 && <p className="text-muted-foreground">No messages yet.</p>}
        {room.chat.map((m) => (
          <p key={m.id} className="leading-relaxed">
            <span className="font-semibold">{m.sender_name}</span> <span className="text-xs text-muted-foreground tabular-nums">{formatTime(m.timestamp)}</span>
            {/* Plain text only: the server's text is never rendered as markup. */}
            <span className="block break-words whitespace-pre-wrap text-white/85 select-text">{m.text}</span>
          </p>
        ))}
      </div>
      <div className="flex items-end gap-2">
        <div className="min-w-0 flex-1">
          <TextField label="Message" value={text} onChange={setText} onEnter={send} placeholder="Say something…" />
        </div>
        <Button variant="glass" size="icon" icon={Send} label="Send" disabled={!text.trim()} onClick={send} />
      </div>
    </div>
  );
}
