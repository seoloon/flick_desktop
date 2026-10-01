// Watch together: create or join a room, then follow it. The room session is
// owned by Rust; this screen only shows `useWatch().room` and sends actions,
// so leaving the screen never leaves the room.
import { Copy, LogOut, MessageCircle, MonitorPlay, Send, Users } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { CenteredSpinner, EmptyState, Notice } from "@/components/tv/Feedback";
import { Page, PageHeader, Panel, Pill } from "@/components/tv/Page";
import { ToggleRow } from "@/components/tv/SettingsList";
import { TextField } from "@/components/tv/TextField";
import { api, asError } from "@/ipc/api";
import type { Participant } from "@/ipc/bindings/Participant";
import type { RoomState } from "@/ipc/bindings/RoomState";
import { cn } from "@/lib/utils";
import { FocusGroup, Screen } from "@/nav/Focusable";
import { messageText, refreshStatus, useWatch } from "./store";

const initials = (name: string) =>
  name
    .trim()
    .split(/\s+/)
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase() ?? "")
    .join("") || "?";

/** What a participant is doing, derived from what the room and the connection say. */
function participantState(p: Participant, room: RoomState): string {
  if (p.presence !== "connected") return "Reconnecting…";
  if (!room.media) return "Waiting";
  return room.playback?.state === "playing" ? "Watching" : "Paused";
}

function ConnectionPill({ room }: { room: RoomState }) {
  switch (room.connection) {
    case "connected":
      return null;
    case "connecting":
      return <Pill>Connecting…</Pill>;
    case "reconnecting":
      return <Pill tone="warn">Reconnecting…</Pill>;
    default:
      return <Pill tone="warn">Disconnected</Pill>;
  }
}

function Lobby() {
  const navigate = useNavigate();
  const [code, setCode] = useState("");
  const [hostOnly, setHostOnly] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(asError(e).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="grid gap-6 md:grid-cols-2">
      <Panel title="Create a room">
        <p className="text-[0.9375rem] leading-relaxed text-muted-foreground">
          You become the host and choose what everyone watches. Friends join with the code, and each plays the title from their own server.
        </p>
        <ToggleRow label="Only I control playback" hint="Others can still chat and leave." checked={hostOnly} onChange={setHostOnly} />
        <FocusGroup className="flex">
          <Button variant="primary" size="lg" icon={Users} disabled={busy} autoFocus onClick={() => void run(() => api.flicksyncCreate(hostOnly))}>
            Create Room
          </Button>
        </FocusGroup>
      </Panel>
      <Panel title="Join a room">
        <TextField label="Room code" value={code} placeholder="ABCD-1234-WXYZ" onChange={setCode} onEnter={() => code.trim() && void run(() => api.flicksyncJoin(code))} />
        <FocusGroup className="flex">
          <Button variant="primary" size="lg" disabled={busy || !code.trim()} onClick={() => void run(() => api.flicksyncJoin(code))}>
            Join
          </Button>
          <Button variant="ghost" size="lg" onClick={() => navigate(-1)}>
            Cancel
          </Button>
        </FocusGroup>
      </Panel>
      {error && <Notice tone="error" className="md:col-span-2">{error}</Notice>}
    </div>
  );
}

function Chat({ room }: { room: RoomState }) {
  const [text, setText] = useState("");
  const endRef = useRef<HTMLDivElement>(null);
  const { setChatOpen } = useWatch.getState();
  useEffect(() => {
    setChatOpen(true);
    return () => setChatOpen(false);
  }, [setChatOpen]);
  useEffect(() => endRef.current?.scrollIntoView({ block: "end" }), [room.chat.length]);

  const send = () => {
    const t = text.trim();
    if (!t) return;
    setText("");
    api.flicksyncChat(t).catch((e) => toast.error(asError(e).message));
  };
  const fmt = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });

  return (
    <Panel title="Chat">
      <div className="flex max-h-72 min-h-24 flex-col gap-2 overflow-y-auto pr-1 text-sm">
        {room.chat.length === 0 && <p className="text-muted-foreground">No messages yet.</p>}
        {room.chat.map((m) => (
          <p key={m.id} className="leading-relaxed">
            <span className="font-semibold">{m.sender_name}</span> <span className="text-xs text-muted-foreground tabular-nums">{fmt.format(m.timestamp)}</span>
            {/* Plain text only: the server's text is never rendered as markup. */}
            <span className="block break-words whitespace-pre-wrap text-white/85 select-text">{m.text}</span>
          </p>
        ))}
        <div ref={endRef} />
      </div>
      <div className="flex items-end gap-2">
        <div className="min-w-0 flex-1">
          <TextField label="Message" value={text} onChange={setText} onEnter={send} placeholder="Say something…" />
        </div>
        <Button variant="glass" size="icon" icon={Send} label="Send" disabled={!text.trim()} onClick={send} />
      </div>
    </Panel>
  );
}

function RoomView({ room }: { room: RoomState }) {
  const isHost = room.hostId === room.you;
  const unread = useWatch((s) => s.unread);
  const [chat, setChat] = useState(false);
  const act = (p: Promise<unknown>) => p.catch((e) => toast.error(asError(e).message));
  const copy = () => navigator.clipboard.writeText(room.shareCode || room.roomId).then(() => toast.success("Room code copied"), () => toast.error("Couldn't copy the code"));

  return (
    <>
      <PageHeader
        title={
          <span className="flex items-center gap-4">
            <span className="tabular-nums">{room.shareCode || room.roomId}</span>
            <ConnectionPill room={room} />
          </span>
        }
        lead="Share this code. Everyone watches from their own server."
        actions={
          <FocusGroup className="flex items-center gap-3">
            <Button icon={Copy} onClick={() => void copy()}>
              Copy Code
            </Button>
            <Button variant="danger" icon={LogOut} onClick={() => void act(api.flicksyncLeave())}>
              Leave
            </Button>
          </FocusGroup>
        }
      />

      <div className="grid gap-6 md:grid-cols-2">
        <Panel title="Participants">
          <ul className="flex flex-col gap-3">
            {room.participants.map((p) => (
              <li key={p.participant_id} className={cn("flex items-center gap-3", p.presence !== "connected" && "opacity-60")}>
                <span className="grid size-10 place-items-center rounded-full bg-white/15 text-sm font-bold">{initials(p.display_name)}</span>
                <span className="min-w-0 flex-1 truncate font-medium">
                  {p.display_name}
                  {p.participant_id === room.you && <span className="text-muted-foreground"> (you)</span>}
                </span>
                {p.is_host && <Pill tone="strong">Host</Pill>}
                <span className="w-28 text-right text-sm text-muted-foreground">{participantState(p, room)}</span>
              </li>
            ))}
          </ul>
        </Panel>

        <Panel title="Now playing">
          {room.media ? (
            <>
              <p className="text-xl font-semibold">{room.media.title ?? "Shared title"}</p>
              <FocusGroup className="flex">
                <Button icon={MonitorPlay} onClick={() => void act(api.flicksyncResyncMedia())}>
                  Open Player
                </Button>
              </FocusGroup>
            </>
          ) : (
            <p className="text-muted-foreground">Nothing yet.</p>
          )}
          <p className="text-sm leading-relaxed text-muted-foreground">
            {isHost
              ? "Open any movie or episode and press Play, or use Watch Together on its page, to share it with the room."
              : "Only the host can choose what the room watches."}
          </p>
          {!isHost && room.controlMode === "host_only" && <p className="text-sm text-muted-foreground">Only the host controls playback in this room.</p>}
        </Panel>

        {isHost && (
          <Panel title="Room settings" className="md:col-span-2">
            <ToggleRow
              label="Everyone can control playback"
              hint="When off, only you can play, pause and seek."
              checked={room.controlMode === "everyone"}
              onChange={(v) => void act(api.flicksyncUpdateRoom(!v, null))}
            />
            <ToggleRow label="Chat" checked={room.chatEnabled} onChange={(v) => void act(api.flicksyncUpdateRoom(null, v))} />
            <FocusGroup className="flex">
              <Button variant="danger" onClick={() => void act(api.flicksyncCloseRoom())}>
                Close Room for Everyone
              </Button>
            </FocusGroup>
          </Panel>
        )}

        {room.chatEnabled && (
          <div className="md:col-span-2">
            {chat ? (
              <Chat room={room} />
            ) : (
              <FocusGroup className="flex">
                <Button icon={MessageCircle} onClick={() => setChat(true)}>
                  Chat{unread > 0 ? ` (${unread})` : room.chat.length > 0 ? ` (${room.chat.length})` : ""}
                </Button>
              </FocusGroup>
            )}
          </div>
        )}
      </div>
    </>
  );
}

export function Watch() {
  const room = useWatch((s) => s.room);
  const status = useWatch((s) => s.status);
  const navigate = useNavigate();
  useEffect(() => void refreshStatus(), []);

  if (!room && !status) return <CenteredSpinner />;
  if (!room && status && !status.configured) {
    return (
      <Screen>
        <EmptyState
          title="Watch together"
          icon={<Users className="size-14 text-white/70" />}
          actions={
            <Button variant="primary" onClick={() => navigate("/settings?s=watch")}>
              Set Up
            </Button>
          }
        >
          Watch the same title at the same time as friends, each from their own server. Add the address of your Flick Server in Settings to turn it on.
        </EmptyState>
      </Screen>
    );
  }

  return (
    <Screen ready>
      <Page>
        {room ? (
          <RoomView room={room} />
        ) : (
          <>
            <PageHeader title="Watch Together" lead="Everyone plays from their own server, in sync." />
            {status && !status.available && <Notice tone="warn">{messageText(status.message ?? "unavailable")}</Notice>}
            <Lobby />
          </>
        )}
      </Page>
    </Screen>
  );
}
