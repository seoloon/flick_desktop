import { afterEach, describe, expect, it, vi } from "vitest";
import { settled } from "./intro";

afterEach(() => {
  vi.useRealTimers();
});

describe("settled", () => {
  it("resolves once resizes have stopped for the quiet period", async () => {
    vi.useFakeTimers();
    const target = new EventTarget();
    let done = false;
    void settled(target, 150, 1000).then(() => (done = true));
    for (let i = 0; i < 5; i++) {
      await vi.advanceTimersByTimeAsync(100);
      target.dispatchEvent(new Event("resize"));
    }
    expect(done).toBe(false);
    await vi.advanceTimersByTimeAsync(149);
    expect(done).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(done).toBe(true);
  });

  it("gives up waiting after the cap", async () => {
    vi.useFakeTimers();
    const target = new EventTarget();
    let done = false;
    void settled(target, 150, 1000).then(() => (done = true));
    for (let i = 0; i < 12; i++) {
      await vi.advanceTimersByTimeAsync(100);
      target.dispatchEvent(new Event("resize"));
    }
    expect(done).toBe(true);
  });
});
