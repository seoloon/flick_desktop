import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProfileAccount } from "@/ipc/bindings/ProfileAccount";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";

vi.mock("@/ipc/api", () => ({
  api: { profileSwitch: vi.fn(async () => ({ failed: [] })) },
  asError: (e: { kind?: string; message?: string }) => ({ kind: e?.kind ?? "other", message: String(e?.message ?? e) }),
}));
vi.mock("@/lib/settings", () => ({ loadSettings: vi.fn(async () => null) }));
vi.mock("@/lib/ambient", () => ({ ambientReset: vi.fn() }));

import { queryClient } from "./queryClient";
import { accountTitle, initials, needsPlexPin, nextStage, pendingSignIns, pinError, switchProfile, useProfileSwitch, visibleProfiles } from "./profiles";

const account = (over: Partial<ProfileAccount> = {}): ProfileAccount => ({
  kind: "jellyfin",
  serverName: "Home",
  userName: "Antoine",
  state: "connected",
  connection: "c1",
  baseUrl: "http://home.local/",
  needsPassword: false,
  plexPin: false,
  ...over,
});
const card = (over: Partial<ProfileCard> = {}): ProfileCard => ({ id: "p1", name: "Antoine", color: "#5e8bff", avatarKey: null, locked: false, hidden: false, accounts: [account()], ...over });

describe("profile helpers", () => {
  it("makes initials from one or two words", () => {
    expect(initials("antoine")).toBe("A");
    expect(initials("  Élodie  Martin ")).toBe("ÉM");
    expect(initials("")).toBe("?");
  });

  it("hides hidden profiles", () => {
    expect(visibleProfiles([card(), card({ id: "p2", hidden: true })]).map((c) => c.id)).toEqual(["p1"]);
  });

  it("finds what must be typed before entering", () => {
    const c = card({ accounts: [account(), account({ state: "pending", connection: null, needsPassword: true }), account({ kind: "plex", plexPin: true })] });
    expect(pendingSignIns(c)).toHaveLength(1);
    expect(needsPlexPin(c)).toBe(true);
    expect(nextStage(c, null)).toBe("plex-pin");
    expect(nextStage({ ...c, locked: true }, null)).toBe("pin");
    expect(nextStage(c, "plex-pin")).toBe("sign-in");
    expect(nextStage(c, "sign-in")).toBe("loading");
    expect(nextStage(card(), null)).toBe("loading");
  });

  it("describes where an account comes from", () => {
    expect(accountTitle(account())).toBe("Antoine on Home (Jellyfin)");
    expect(accountTitle(account({ state: "pending" }))).toBe("Antoine on Home (Jellyfin) · sign-in needed");
    expect(accountTitle(account({ kind: "plex", state: "offline" }))).toBe("Antoine on Home (Plex) · offline");
    expect(accountTitle(account({ state: "disabled" }))).toBe("Antoine on Home (Jellyfin) · off");
  });

  it("reads PIN errors", () => {
    expect(pinError({ kind: "wrongPin", message: "wrong PIN" })).toBe("wrong");
    expect(pinError({ kind: "pinLocked", message: "too many attempts, try again in 42 s" })).toEqual({ locked: 42 });
    expect(pinError({ kind: "network", message: "down" })).toBeNull();
  });
});

describe("switchProfile", () => {
  beforeEach(() => useProfileSwitch.setState({ phase: "idle", target: null }));

  it("drops every cached query of the previous profile", async () => {
    queryClient.setQueryData(["home"], { rows: ["previous user's row"] });
    queryClient.setQueryData(["item", "x"], { title: "theirs" });
    await switchProfile("p2", {});
    expect(queryClient.getQueryData(["home"])).toBeUndefined();
    expect(queryClient.getQueryData(["item", "x"])).toBeUndefined();
    expect(useProfileSwitch.getState().phase).toBe("entering");
  });

  it("goes back to idle when the switch fails", async () => {
    const { api } = await import("@/ipc/api");
    vi.mocked(api.profileSwitch).mockRejectedValueOnce({ kind: "wrongPin", message: "wrong PIN" });
    await expect(switchProfile("p2", { pin: "0000" })).rejects.toBeTruthy();
    expect(useProfileSwitch.getState().phase).toBe("idle");
  });
});
