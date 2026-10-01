// The room, inside the player: a card over the controls with two tabs,
// Management (code, participants, host settings, leave) and Chat.
import { Copy, LogOut } from "lucide-react";
import { motion } from "motion/react";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { api } from "@/ipc/api";
import type { RoomState } from "@/ipc/bindings/RoomState";
import { focusSpring, panelSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { act, ChatView, HostControls, isHost, ParticipantsList } from "./RoomParts";
import { useWatch } from "./store";

export type RoomTab = "management" | "chat";

function Tab({ label, selected, badge, onSelect }: { label: string; selected: boolean; badge?: number; onSelect: () => void }) {
  const tv = useTv<HTMLButtonElement>({ scroll: "nearest" });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      role="tab"
      aria-selected={selected}
      onClick={onSelect}
      animate={{ scale: tv.showFocus ? 1.04 : 1 }}
      transition={focusSpring}
      className={cn(
        "flex h-8 shrink-0 cursor-pointer items-center gap-1.5 rounded-full px-3.5 text-xs font-semibold whitespace-nowrap transition-colors",
        tv.showFocus ? "bg-white text-black" : selected ? "bg-white/20 text-white" : "text-white/60 hover:bg-white/10 hover:text-white",
      )}
    >
      {label}
      {!!badge && <span className="grid min-w-4 place-items-center rounded-full bg-white px-1 text-[0.625rem] leading-4 font-bold text-black">{badge}</span>}
    </motion.button>
  );
}

export function RoomPanel({ room, tab, onTab }: { room: RoomState; tab: RoomTab; onTab: (t: RoomTab) => void }) {
  const unread = useWatch((s) => s.unread);
  const copy = () =>
    navigator.clipboard.writeText(room.shareCode || room.roomId).then(
      () => toast.success("Room code copied"),
      () => toast.error("Couldn't copy the code"),
    );

  return (
    <motion.aside
      aria-label="Watch room"
      initial={{ opacity: 0, y: 12, scale: 0.94 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 8, scale: 0.96 }}
      transition={panelSpring}
      style={{ transformOrigin: "bottom right" }}
      className="absolute right-8 bottom-[calc(100%+0.25rem)] flex w-[min(26rem,calc(100vw-4rem))] flex-col gap-2 overflow-hidden rounded-2xl bg-black/75 p-3 text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.12),inset_0_0_0_1px_rgb(255_255_255/0.08),0_30px_80px_-20px_rgb(0_0_0/0.8)]"
    >
      <FocusGroup focusKey="player-room" boundary className="flex flex-col gap-3">
        <FocusGroup role="tablist" aria-label="Room" className="flex gap-1">
          <Tab label="Management" selected={tab === "management"} onSelect={() => onTab("management")} />
          {room.chatEnabled && <Tab label="Chat" selected={tab === "chat"} badge={tab === "chat" ? 0 : unread} onSelect={() => onTab("chat")} />}
        </FocusGroup>

        {tab === "chat" && room.chatEnabled ? (
          <ChatView room={room} />
        ) : (
          <div className="flex max-h-[50vh] flex-col gap-4 overflow-y-auto pr-1">
            <div className="flex items-center justify-between gap-3">
              <div className="flex flex-col">
                <span className="text-xs text-white/55">Room code</span>
                <span className="text-lg font-semibold tabular-nums">{room.shareCode || room.roomId}</span>
              </div>
              <Button variant="glass" size="sm" icon={Copy} onClick={() => void copy()}>
                Copy
              </Button>
            </div>
            <ParticipantsList room={room} />
            <HostControls room={room} />
            {!isHost(room) && <p className="text-xs leading-relaxed text-white/55">Only the host can choose what the room watches{room.controlMode === "host_only" ? " and control playback" : ""}.</p>}
            <FocusGroup className="flex">
              <Button variant="danger" size="sm" icon={LogOut} onClick={() => void act(api.flicksyncLeave())}>
                Leave Room
              </Button>
            </FocusGroup>
          </div>
        )}
      </FocusGroup>
    </motion.aside>
  );
}
