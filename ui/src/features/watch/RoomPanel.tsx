// The room, inside the player: a card over the controls showing either the
// room (code, participants, host settings, leave) or the chat. Which one is
// decided by the player's two buttons, Room and Chat.
import { Copy, LogOut } from "lucide-react";
import { motion } from "motion/react";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { api } from "@/ipc/api";
import type { RoomState } from "@/ipc/bindings/RoomState";
import { panelSpring } from "@/lib/motion";
import { FocusGroup } from "@/nav/Focusable";
import { act, ChatView, HostControls, isHost, ParticipantsList } from "./RoomParts";
import { withCode, UI_CODE } from "@/lib/errors";

export type RoomTab = "management" | "chat";

/** The room card; `tab` is chosen by the player's Room and Chat buttons. */
export function RoomPanel({ room, tab }: { room: RoomState; tab: RoomTab }) {
  const chat = tab === "chat" && room.chatEnabled;
  const copy = () =>
    navigator.clipboard.writeText(room.shareCode || room.roomId).then(
      () => toast.success("Room code copied"),
      () => toast.error(withCode("Couldn't copy the code", UI_CODE.clipboard)),
    );

  return (
    <motion.aside
      aria-label={chat ? "Room chat" : "Watch room"}
      initial={{ opacity: 0, y: 12, scale: 0.94 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 8, scale: 0.96 }}
      transition={panelSpring}
      style={{ transformOrigin: "bottom right" }}
      className="absolute right-8 bottom-[calc(100%+0.25rem)] flex w-[min(26rem,calc(100vw-4rem))] flex-col gap-2 overflow-hidden rounded-2xl bg-black/75 p-3 text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.12),inset_0_0_0_1px_rgb(255_255_255/0.08),0_30px_80px_-20px_rgb(0_0_0/0.8)]"
    >
      <FocusGroup focusKey="player-room" boundary className="flex flex-col gap-3">
        <h2 className="px-1 text-xs font-semibold text-white/60">{chat ? "Chat" : "Room"}</h2>

        {chat ? (
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
