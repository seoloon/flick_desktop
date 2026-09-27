import { afterEach, describe, expect, it, vi } from "vitest";
import { fullscreenSized } from "./intro";

afterEach(() => {
  vi.useRealTimers();
});

describe("fullscreenSized", () => {
  it("waits for the window to reach the screen's size, however late the resize comes", async () => {
    vi.useFakeTimers();
    const target = new EventTarget();
    let full = false;
    let done = false;
    void fullscreenSized(target, () => full, 1500).then(() => (done = true));
    await vi.advanceTimersByTimeAsync(400);
    target.dispatchEvent(new Event("resize"));
    await vi.advanceTimersByTimeAsync(0);
    expect(done).toBe(false);
    full = true;
    target.dispatchEvent(new Event("resize"));
    await vi.advanceTimersByTimeAsync(0);
    expect(done).toBe(true);
  });

  it("resolves at once when the window already has that size", async () => {
    let done = false;
    void fullscreenSized(new EventTarget(), () => true, 1500).then(() => (done = true));
    await Promise.resolve();
    expect(done).toBe(true);
  });

  it("gives up waiting after the cap", async () => {
    vi.useFakeTimers();
    let done = false;
    void fullscreenSized(new EventTarget(), () => false, 1500).then(() => (done = true));
    await vi.advanceTimersByTimeAsync(1499);
    expect(done).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(done).toBe(true);
  });
});
