import { describe, expect, it } from "vitest";
import { bitrate, channelsLabel, clock, duration, resolutionLabel } from "./format";

describe("time formatting", () => {
  it("formats runtimes in hours and minutes", () => {
    expect(duration(92 * 60_000)).toBe("1 h 32");
    expect(duration(44 * 60_000)).toBe("44 min");
    expect(duration(0)).toBe("");
  });

  it("formats player clocks", () => {
    expect(clock(624_000)).toBe("10:24");
    expect(clock(3_725_000)).toBe("1:02:05");
    expect(clock(-5)).toBe("0:00");
  });
});

describe("technical labels", () => {
  it("names resolutions by class, not exact size", () => {
    expect(resolutionLabel(3840, 1600)).toBe("4K");
    expect(resolutionLabel(1920, 800)).toBe("1080p");
    expect(resolutionLabel(720, 576)).toBe("SD");
  });

  it("names channel layouts", () => {
    expect(channelsLabel(6)).toBe("5.1");
    expect(channelsLabel(3)).toBe("3 ch");
  });

  it("formats bitrates", () => {
    expect(bitrate(40_000_000)).toBe("40.0 Mb/s");
    expect(bitrate(640_000)).toBe("640 kb/s");
    expect(bitrate(null)).toBe("");
  });
});
