import { describe, expect, it } from "vitest";
import type { DownloadItem } from "@/ipc/app-types";
import { bytesText, percent, sortDownloads, stateText } from "./format";

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
    expect(stateText(item({ offset: 1024, size: 4096, speed: 2048 }))).toBe("1.0 KB of 4.0 KB · 2.0 KB/s");
    expect(stateText(item({ note: "Connection problem, retrying (3 s)" }))).toBe("Connection problem, retrying (3 s)");
    expect(stateText(item({ state: "paused", offset: 50, size: 100 }))).toBe("Paused · 50 %");
    expect(stateText(item({ state: "failed", error: "No." }))).toBe("No.");
    expect(stateText(item({ state: "done", size: 1024, missing: true }))).toBe("The file is no longer there");
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
