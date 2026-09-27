// Settings store: loaded once, edited in place, persisted (debounced) to Rust
// which applies runtime-changeable options to the player immediately.
import { create } from "zustand";
import { api, asError } from "@/ipc/api";
import type { Settings } from "@/ipc/bindings/Settings";

type SettingsState = { settings: Settings | null; saveError: string | null };

export const useSettingsStore = create<SettingsState>(() => ({ settings: null, saveError: null }));

/** The loaded settings; screens render only after boot, so this is non-null in practice. */
export function useSettings(): Settings | null {
  return useSettingsStore((s) => s.settings);
}

export function getSettings(): Settings | null {
  return useSettingsStore.getState().settings;
}

let saveTimer: number | undefined;

export async function loadSettings(): Promise<Settings> {
  const s = await api.settingsGet();
  useSettingsStore.setState({ settings: s });
  applyAppearance(s);
  return s;
}

/** Saves pending changes now, for actions that need Rust to see them (a quality change reloads playback). */
export async function flushSettings() {
  const current = getSettings();
  window.clearTimeout(saveTimer);
  if (current) await api.settingsSet(current);
}

/** Mutates a copy of the settings and schedules a save. */
export function updateSettings(fn: (s: Settings) => void) {
  const current = getSettings();
  if (!current) return;
  const next = structuredClone(current);
  fn(next);
  useSettingsStore.setState({ settings: next });
  applyAppearance(next);
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    api.settingsSet(next).then(
      () => useSettingsStore.setState({ saveError: null }),
      (e) => {
        useSettingsStore.setState({ saveError: asError(e).message });
        console.error("settings save failed", e);
      },
    );
  }, 400);
}

/** Appearance settings live on the root element as attributes and variables. */
function applyAppearance(s: Settings) {
  const a = s.appearance;
  const root = document.documentElement;
  root.style.setProperty("--ambient-strength", String(a.backgroundIntensity));
  root.dataset.density = a.density;
  root.dataset.blur = String(a.blur);
}
