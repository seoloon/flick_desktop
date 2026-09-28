import { describe, expect, it } from "vitest";
import { ago, formatBytes, parseNotes } from "./updates";

describe("parseNotes", () => {
  it("reads GitHub's generated notes", () => {
    const md = [
      "## What's Changed",
      "* Auto updates by @seoloon in https://github.com/seoloon/flick_desktop/pull/3",
      "* **Faster** `home` rows",
      "",
      "**Full Changelog**: https://github.com/seoloon/flick_desktop/compare/v0.1.0...v0.2.0",
    ].join("\r\n");
    expect(parseNotes(md)).toEqual([
      { kind: "heading", text: "What's Changed" },
      { kind: "item", text: "Auto updates by @seoloon in https://github.com/seoloon/flick_desktop/pull/3" },
      { kind: "item", text: "Faster home rows" },
      { kind: "text", text: "Full Changelog: https://github.com/seoloon/flick_desktop/compare/v0.1.0...v0.2.0" },
    ]);
  });

  it("joins wrapped lines into one paragraph and keeps link labels", () => {
    expect(parseNotes("First line\nsecond [line](https://x.y).\n\n---\nNext")).toEqual([
      { kind: "text", text: "First line second line." },
      { kind: "text", text: "Next" },
    ]);
  });

  it("drops empty blocks", () => {
    expect(parseNotes("![shot](a.png)\n\n1. One")).toEqual([{ kind: "item", text: "One" }]);
  });
});

describe("formatBytes", () => {
  it("uses KB below a megabyte, MB above", () => {
    expect(formatBytes(10)).toBe("1 KB");
    expect(formatBytes(512 * 1024)).toBe("512 KB");
    expect(formatBytes(36.4 * 1024 * 1024)).toBe("36.4 MB");
  });
});

describe("ago", () => {
  const now = 1_000_000_000;
  it("is relative to now", () => {
    expect(ago(now - 10_000, now)).toBe("just now");
    expect(ago(now - 5 * 60_000, now)).toBe("5 minutes ago");
    expect(ago(now - 3 * 3_600_000, now)).toBe("3 hours ago");
    expect(ago(now - 26 * 3_600_000, now)).toBe("yesterday");
  });
});
