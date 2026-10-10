// Watch together: create or join a room, then follow it. The room session is
// owned by Rust; this screen only shows `useWatch().room` and sends actions,
// so leaving the screen never leaves the room.
import { Copy, LogOut, MessageCircle, MonitorPlay, Users } from "lucide-react";
import { useState } from "react";
import { useNavigate } from "react-router";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { CenteredSpinner, EmptyState, Notice } from "@/components/tv/Feedback";
import { Page, PageHeader, Panel, Pill } from "@/components/tv/Page";
import { ToggleRow } from "@/components/tv/SettingsList";
import { TextField } from "@/components/tv/TextField";
import { api } from "@/ipc/api";
import type { RoomState } from "@/ipc/bindings/RoomState";
import { FocusGroup, Screen } from "@/nav/Focusable";
import { act, ChatView, HostControls, isHost, ParticipantsList } from "./RoomParts";
import { messageText, useWatch } from "./store";
import { UI_CODE, errorText, withCode } from "@/lib/errors";

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
      setError(errorText(e));
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

function RoomView({ room }: { room: RoomState }) {
  const host = isHost(room);
  const unread = useWatch((s) => s.unread);
  const [chat, setChat] = useState(false);
  const copy = () => navigator.clipboard.writeText(room.shareCode || room.roomId).then(() => toast.success("Room code copied"), () => toast.error(withCode("Couldn't copy the code", UI_CODE.clipboard)));

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
          <ParticipantsList room={room} />
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
            {host
              ? "Open any movie or episode and press Play, or use Watch Together on its page, to share it with the room."
              : "Only the host can choose what the room watches."}
          </p>
          {!host && room.controlMode === "host_only" && <p className="text-sm text-muted-foreground">Only the host controls playback in this room.</p>}
        </Panel>

        {host && (
          <Panel title="Room settings" className="md:col-span-2">
            <HostControls room={room} />
          </Panel>
        )}

        {room.chatEnabled && (
          <div className="md:col-span-2">
            {chat ? (
              <Panel title="Chat">
                <ChatView room={room} />
              </Panel>
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

  if (!room && !status) return <CenteredSpinner />;
  if (!room && status && !status.configured) {
    return (
      <Screen>
        <EmptyState
          title="Watch together"
          icon={<Users className="size-14 text-white/70" />}
          actions={
            <Button variant="primary" onClick={() => navigate("/settings?s=flickserver&tab=watch")}>
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
