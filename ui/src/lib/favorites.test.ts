import { describe, expect, it, vi } from "vitest";
import type { MediaItem } from "@/ipc/bindings/MediaItem";

vi.mock("@/ipc/api", () => ({ api: {} }));

import { favoriteShelves } from "./favorites";

const item = (id: string, kind: MediaItem["kind"]) => ({ id, kind }) as MediaItem;

describe("favoriteShelves", () => {
  it("groups by kind in a fixed order and skips empty shelves", () => {
    const shelves = favoriteShelves([item("e1", "episode"), item("m1", "movie"), item("m2", "movie"), item("v1", "musicVideo")]);
    expect(shelves.map((s) => [s.title, s.items.map((i) => i.id)])).toEqual([
      ["Movies", ["m1", "m2"]],
      ["Episodes", ["e1"]],
      ["Other", ["v1"]],
    ]);
    expect(shelves.find((s) => s.title === "Episodes")?.shape).toBe("thumb");
  });

  it("is empty without favourites", () => {
    expect(favoriteShelves([])).toEqual([]);
  });
});
