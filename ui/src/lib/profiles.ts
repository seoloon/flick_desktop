// Multi-user profiles on the UI side: the state query, the switch
// sequence (fade out, swap in Rust, drop every cached query, fade in) and
// small pure helpers the screens share.
import { create } from "zustand";
import { api, asError } from "@/ipc/api";
import type { SwitchOutcome } from "@/ipc/app-types";
import type { ProfileAccount } from "@/ipc/bindings/ProfileAccount";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import type { ProfileId } from "@/ipc/bindings/ProfileId";
import { ambientReset } from "@/lib/ambient";
import { loadSettings } from "@/lib/settings";
import { queryClient } from "./queryClient";

export const profilesQuery = { queryKey: ["profiles"], queryFn: () => api.profilesState() };

/** Every connection on this computer (profile sheets, "Other User"); the
 * plain `serversQuery` lists only the active profile's. Invalidating
 * `["servers"]` refreshes both. */
export const allServersQuery = { queryKey: ["servers", "all"], queryFn: () => api.serversList(true) };

/** Mirrors `PROFILE_COLORS` in crates/storage/src/profiles.rs. */
export const PROFILE_COLORS = ["#5e8bff", "#ff6b6b", "#3ecf8e", "#ffb547", "#b07cff", "#ff7ac6", "#35c6d6", "#a3a3a3"];

type SwitchPhase = "idle" | "leaving" | "entering";
export const useProfileSwitch = create<{ phase: SwitchPhase; target: ProfileId | null }>(() => ({ phase: "idle", target: null }));

/** Content fade before the swap (the Shell animates on `phase`). */
const LEAVE_MS = 200;
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function switchProfile(id: ProfileId, opts: { pin?: string; plexPin?: string }): Promise<SwitchOutcome> {
  useProfileSwitch.setState({ phase: "leaving", target: id });
  try {
    const [outcome] = await Promise.all([api.profileSwitch(id, opts.pin ?? null, opts.plexPin ?? null), wait(LEAVE_MS)]);
    // Never show one frame of the previous profile's data.
    queryClient.clear();
    ambientReset();
    await loadSettings();
    useProfileSwitch.setState({ phase: "entering" });
    return outcome;
  } catch (e) {
    useProfileSwitch.setState({ phase: "idle", target: null });
    throw e;
  }
}

/** Called once the new profile's screen has faded in. */
export function finishSwitch() {
  useProfileSwitch.setState({ phase: "idle", target: null });
}

export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  return words
    .slice(0, 2)
    .map((w) => w[0]!.toLocaleUpperCase())
    .join("");
}

export function avatarGradient(color: string): string {
  return `radial-gradient(120% 120% at 30% 20%, color-mix(in srgb, ${color} 80%, white), ${color} 45%, color-mix(in srgb, ${color} 55%, black))`;
}

export const visibleProfiles = (cards: ProfileCard[]) => cards.filter((c) => !c.hidden);
export const needsPlexPin = (card: ProfileCard) => card.accounts.some((a) => a.plexPin);
export const pendingSignIns = (card: ProfileCard) => card.accounts.filter((a) => a.state === "pending" && a.kind === "jellyfin" && a.needsPassword);

export function accountTitle(a: ProfileAccount): string {
  const base = `${a.userName} on ${a.serverName} (${a.kind === "plex" ? "Plex" : "Jellyfin"})`;
  if (a.state === "disabled") return `${base} · off`;
  if (a.state === "pending") return `${base} · sign-in needed`;
  if (a.state === "offline") return `${base} · offline`;
  return base;
}

export function pinError(e: unknown): "wrong" | { locked: number } | null {
  const err = asError(e);
  if (err.kind === "wrongPin") return "wrong";
  if (err.kind === "pinLocked") return { locked: Number(/(\d+)/.exec(err.message)?.[1] ?? 30) };
  return null;
}

export type PickStage = "pin" | "plex-pin" | "sign-in" | "loading";
const STAGES: PickStage[] = ["pin", "plex-pin", "sign-in", "loading"];

/** What comes after `after` when entering `card` (Flick PIN, Plex PIN, sign-ins, then load). */
export function nextStage(card: ProfileCard, after: PickStage | null): PickStage {
  const needed: Record<PickStage, boolean> = {
    pin: card.locked,
    "plex-pin": needsPlexPin(card),
    "sign-in": pendingSignIns(card).length > 0,
    loading: true,
  };
  const from = after ? STAGES.indexOf(after) + 1 : 0;
  return STAGES.slice(from).find((s) => needed[s]) ?? "loading";
}
