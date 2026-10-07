import { describe, expect, it } from "vitest";
import type { DownloadItem } from "@/ipc/app-types";
import { bytesText, etaText, percent, remainingSecs, totalRemainingSecs, sortDownloads, stateText } from "./format";

const item = (o: Partial<DownloadItem>): DownloadItem => ({
  id: "a",
  backend: "jellyfin",
  itemId: "x",
  itemRef: "s:x",
  title: "The Long Night",
  subtitle: null,
  kind: "movie",
  state: "active",
  offset: 0,
  size: null,
  filename: null,
  finalPath: null,
  error: null,
  note: null,
  speed: null,
  missing: false,
  createdMs: 0,
  ...o,
});

describe("download text", () => {
  it("formats sizes", () => {
    expect(bytesText(0)).toBe("0 B");
    expect(bytesText(1536)).toBe("1.5 KB");
    expect(bytesText(4_831_838_208)).toBe("4.5 GB");
    expect(bytesText(250 * 1024 * 1024)).toBe("250 MB");
  });

  it("computes progress", () => {
    expect(percent(item({ offset: 50, size: 200 }))).toBe(25);
    expect(percent(item({ offset: 0, size: null }))).toBe(0);
    expect(percent(item({ state: "done", offset: 0, size: 5 }))).toBe(100);
    expect(percent(item({ offset: 999, size: 100 }))).toBe(100);
  });

  it("says what is going on", () => {
    expect(stateText(item({ state: "queued" }))).toBe("Waiting to start");
    expect(stateText(item({ offset: 1024, size: 4096, speed: 2048 }))).toBe("1.0 KB of 4.0 KB · 2.0 KB/s · 2 s left");
    expect(stateText(item({ note: "Connection problem, retrying (3 s)" }))).toBe("Connection problem, retrying (3 s)");
    expect(stateText(item({ state: "paused", offset: 50, size: 100 }))).toBe("Paused · 50 %");
    expect(stateText(item({ state: "failed", error: "No." }))).toBe("No.");
    expect(stateText(item({ state: "done", size: 1024, missing: true }))).toBe("The file is no longer there");
  });

  it("estimates the time left", () => {
    expect(etaText(0)).toBe("1 s");
    expect(etaText(45)).toBe("45 s");
    expect(etaText(61)).toBe("2 min");
    expect(etaText(3900)).toBe("1 h 05");
    expect(remainingSecs(item({ offset: 100, size: 1100, speed: 100 }))).toBe(10);
    expect(remainingSecs(item({ offset: 100, size: 1100, speed: null }))).toBeNull();
    expect(remainingSecs(item({ state: "paused", offset: 100, size: 1100, speed: 100 }))).toBeNull();
  });

  it("estimates the time left for the whole queue", () => {
    const running = item({ id: "a", offset: 0, size: 1000, speed: 100 });
    const waiting = item({ id: "b", state: "queued", size: 1000 });
    expect(totalRemainingSecs([running, waiting])).toBe(20);
    expect(totalRemainingSecs([item({ id: "c", state: "done", size: 5 })])).toBeNull();
    expect(totalRemainingSecs([running, item({ id: "d", state: "queued", size: null })])).toBeNull();
    expect(totalRemainingSecs([waiting])).toBeNull();
  });

  it("lists running downloads first", () => {
    const sorted = sortDownloads([
      item({ id: "d", state: "done", createdMs: 9 }),
      item({ id: "q", state: "queued", createdMs: 1 }),
      item({ id: "a2", state: "active", createdMs: 1 }),
      item({ id: "a1", state: "active", createdMs: 5 }),
    ]);
    expect(sorted.map((d) => d.id)).toEqual(["a1", "a2", "q", "d"]);
  });
});
