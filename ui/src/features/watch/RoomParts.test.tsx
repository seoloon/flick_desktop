// Server-side render of the room pieces with realistic (and hostile) data:
// a render exception here would blank the whole app in the real window.
import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { RoomState } from "@/ipc/bindings/RoomState";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: class {} }));

const room = (over: Partial<RoomState> = {}): RoomState => ({
  roomId: "R",
  shareCode: "R-1",
  hostId: "alice",
  you: "bob",
  participants: [
    { participant_id: "alice", display_name: "Alice", presence: "connected", is_host: true },
    { participant_id: "bob", display_name: "", presence: "reconnecting", is_host: false },
  ],
  media: null,
  playback: null,
  controlMode: "everyone",
  chatEnabled: true,
  connection: "connected",
  chat: [],
  closedReason: null,
  ...over,
});

describe("room pieces render without throwing", () => {
  it("chat with ordinary, empty and out-of-range timestamps", async () => {
    const { ChatView } = await import("./RoomParts");
    const chat = [
      { id: 1, sender_id: "a", sender_name: "Alice", text: "hello <b>x</b>", timestamp: 1790887546683 },
      { id: 2, sender_id: "a", sender_name: "", text: "", timestamp: Number.NaN },
      { id: 3, sender_id: "a", sender_name: "Zed", text: "x".repeat(2000), timestamp: 1e300 },
      { id: 4, sender_id: "a", sender_name: "Neg", text: "é😀", timestamp: -5 },
    ];
    const html = renderToString(<ChatView room={room({ chat })} />);
    expect(html).toContain("hello &lt;b&gt;x&lt;/b&gt;"); // plain text, never markup
  });

  it("participants, host controls and chat disabled", async () => {
    const { ParticipantsList, HostControls, ChatView } = await import("./RoomParts");
    expect(renderToString(<ParticipantsList room={room()} />)).toContain("Alice");
    expect(() => renderToString(<HostControls room={room({ hostId: "bob" })} />)).not.toThrow();
    expect(renderToString(<ChatView room={room({ chatEnabled: false })} />)).toContain("turned off");
  });
});
