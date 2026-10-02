import { describe, expect, it } from "vitest";
import type { RoomState } from "@/ipc/bindings/RoomState";
import { bannerFor } from "./ChatBanner";

const msg = (id: number, from = "alice") => ({ id, sender_id: from, sender_name: from, text: `m${id}`, timestamp: 0 });

const room = (chat: ReturnType<typeof msg>[], over: Partial<RoomState> = {}): RoomState => ({
  roomId: "R",
  shareCode: "R-1",
  hostId: "alice",
  you: "bob",
  participants: [],
  media: null,
  playback: null,
  controlMode: "everyone",
  chatEnabled: true,
  connection: "connected",
  chat,
  closedReason: null,
  ...over,
});

describe("chat banner", () => {
  it("treats the chat already there when the player opens as history", () => {
    expect(bannerFor(room([msg(1), msg(2)]), undefined, false)).toEqual({ seen: 2, show: null });
  });

  it("shows a new message from someone else, once", () => {
    const first = bannerFor(room([msg(1)]), 1, false);
    expect(first.show).toBeNull();
    const news = bannerFor(room([msg(1), msg(2)]), 1, false);
    expect(news.show?.id).toBe(2);
    expect(bannerFor(room([msg(1), msg(2)]), news.seen, false).show).toBeNull();
  });

  it("shows only the latest when several arrive at once", () => {
    expect(bannerFor(room([msg(1), msg(2), msg(3)]), 1, false).show?.id).toBe(3);
  });

  it("stays quiet for your own messages, an open chat and a chat turned off", () => {
    expect(bannerFor(room([msg(1), msg(2, "bob")]), 1, false)).toEqual({ seen: 2, show: null });
    expect(bannerFor(room([msg(1), msg(2)]), 1, true)).toEqual({ seen: 2, show: null });
    expect(bannerFor(room([msg(1), msg(2)], { chatEnabled: false }), 1, false)).toEqual({ seen: 2, show: null });
  });

  it("follows a full chat history where the count no longer grows", () => {
    expect(bannerFor(room([msg(51), msg(52)]), 51, false).show?.id).toBe(52);
  });

  it("handles leaving the room and an empty chat", () => {
    expect(bannerFor(null, 5, false)).toEqual({ seen: null, show: null });
    expect(bannerFor(room([]), undefined, false)).toEqual({ seen: null, show: null });
  });
});
