// Typed IPC surface. Every call maps 1:1 to a `#[tauri::command]` in app/src/commands.
import { invoke } from "@tauri-apps/api/core";
import type { Adjacent } from "./bindings/Adjacent";
import type { Aggregated } from "./bindings/Aggregated";
import type { CapabilityReport } from "./bindings/CapabilityReport";
import type { HomeRow } from "./bindings/HomeRow";
import type { ImageKind } from "./bindings/ImageKind";
import type { ItemKind } from "./bindings/ItemKind";
import type { ItemQuery } from "./bindings/ItemQuery";
import type { ItemRef } from "./bindings/ItemRef";
import type { LiveStats } from "./bindings/LiveStats";
import type { Marker } from "./bindings/Marker";
import type { MediaItem } from "./bindings/MediaItem";
import type { Page } from "./bindings/Page";
import type { PlaybackDecision } from "./bindings/PlaybackDecision";
import type { PlayerCommand } from "./bindings/PlayerCommand";
import type { PlayerSnapshot } from "./bindings/PlayerSnapshot";
import type { PlayRequest } from "./bindings/PlayRequest";
import type { ServerDescriptor } from "./bindings/ServerDescriptor";
import type { ServerId } from "./bindings/ServerId";
import type { ServerLibraries } from "./bindings/ServerLibraries";
import type { ServerStatus } from "./bindings/ServerStatus";
import type { Settings } from "./bindings/Settings";
import type { ProfileId } from "./bindings/ProfileId";
import type { ProfileMode } from "./bindings/ProfileMode";
import type { ProfilesState } from "./bindings/ProfilesState";
import type {
  AboutInfo,
  AdminOverview,
  AppError,
  CssRect,
  LogEntry,
  Palette,
  PlexPin,
  PlexServerChoice,
  ProbeResult,
  QuickConnect,
  RustResult,
  ServerEntry,
  ProfileEdit,
  SwitchOutcome,
} from "./app-types";

/** Normalises a command rejection into `AppError`. */
export function asError(e: unknown): AppError {
  if (e && typeof e === "object" && "message" in e) {
    const o = e as { kind?: unknown; message: unknown };
    return { kind: typeof o.kind === "string" ? o.kind : "other", message: String(o.message) };
  }
  return { kind: "other", message: String(e) };
}

export function unwrap<T, E>(r: RustResult<T, E>): { ok: true; value: T } | { ok: false; error: E } {
  return "Ok" in r ? { ok: true, value: r.Ok } : { ok: false, error: r.Err };
}

const call = <T>(cmd: string, args?: Record<string, unknown>) => invoke<T>(cmd, args);

export const api = {
  // profiles
  profilesState: () => call<ProfilesState>("profiles_state"),
  profilesDiscover: () => call<ProfilesState>("profiles_discover"),
  profilesConfigure: (enabled: boolean, mode: ProfileMode, askOnStartup: boolean, pin: string | null) =>
    call<ProfilesState>("profiles_configure", { enabled, mode, askOnStartup, pin }),
  profileCheckPin: (id: ProfileId, pin: string) => call<void>("profile_check_pin", { id, pin }),
  profileSwitch: (id: ProfileId, pin: string | null, plexPin: string | null) => call<SwitchOutcome>("profile_switch", { id, pin, plexPin }),
  profileCreate: (name: string, color: string) => call<ProfileId>("profile_create", { name, color }),
  /** `ownerPin`: linking a connection another protected profile uses needs its PIN. */
  profileUpdate: (id: ProfileId, edit: ProfileEdit, pin: string | null, ownerPin: string | null) => call<void>("profile_update", { id, edit, pin, ownerPin }),
  profileSetPin: (id: ProfileId, current: string | null, next: string | null) => call<void>("profile_set_pin", { id, current, next }),
  profileDetach: (id: ProfileId, connection: ServerId, pin: string | null) => call<void>("profile_detach", { id, connection, pin }),
  profileDelete: (id: ProfileId, pin: string | null) => call<ServerId[]>("profile_delete", { id, pin }),

  // servers
  serversList: (all = false) => call<ServerEntry[]>("servers_list", { all }),
  serverStatus: (id: ServerId) => call<ServerStatus>("server_status", { id }),
  serverRemove: (id: ServerId) => call<void>("server_remove", { id }),
  serverSetEnabled: (id: ServerId, enabled: boolean) => call<void>("server_set_enabled", { id, enabled }),
  jellyfinProbe: (address: string) => call<ProbeResult>("jellyfin_probe", { address }),
  jellyfinLogin: (url: string, username: string, password: string) =>
    call<ServerDescriptor>("jellyfin_login", { url, username, password }),
  jellyfinQuickConnectStart: (url: string) => call<QuickConnect>("jellyfin_quick_connect_start", { url }),
  jellyfinQuickConnectPoll: (url: string, secret: string) =>
    call<ServerDescriptor | null>("jellyfin_quick_connect_poll", { url, secret }),
  plexPinStart: () => call<PlexPin>("plex_pin_start"),
  plexPinPoll: (id: number) => call<PlexServerChoice[] | null>("plex_pin_poll", { id }),
  plexAddServers: (machineIds: string[]) => call<ServerDescriptor[]>("plex_add_servers", { machineIds }),

  // catalogue
  home: () => call<Aggregated<HomeRow[]>>("home"),
  libraries: () => call<Aggregated<ServerLibraries[]>>("libraries"),
  items: (query: ItemQuery) => call<Page<MediaItem>>("items", { query }),
  item: (id: ItemRef) => call<MediaItem>("item", { id }),
  itemCached: (id: ItemRef) => call<MediaItem | null>("item_cached", { id }),
  children: (id: ItemRef, kind: ItemKind) => call<MediaItem[]>("children", { id, kind }),
  similar: (id: ItemRef) => call<MediaItem[]>("similar", { id }),
  adjacent: (id: ItemRef) => call<Adjacent>("adjacent", { id }),
  markers: (id: ItemRef) => call<Marker[]>("markers", { id }),
  search: (term: string) => call<Aggregated<MediaItem[]>>("search", { term }),
  setPlayed: (id: ItemRef, played: boolean) => call<void>("set_played", { id, played }),
  setFavorite: (id: ItemRef, favorite: boolean) => call<void>("set_favorite", { id, favorite }),

  // playback
  play: (request: PlayRequest) => call<PlaybackDecision>("play", { request }),
  playerReload: () => call<PlaybackDecision>("player_reload"),
  playerCommand: (command: PlayerCommand) => call<void>("player_command", { command }),
  playerViewport: (rect: CssRect) => call<void>("player_viewport", { rect }),
  playerSnapshot: () => call<PlayerSnapshot>("player_snapshot"),
  playerStats: () => call<LiveStats>("player_stats"),

  // system
  settingsGet: () => call<Settings>("settings_get"),
  settingsSet: (settings: Settings) => call<void>("settings_set", { settings }),
  capabilities: (refresh: boolean) => call<CapabilityReport>("capabilities", { refresh }),
  about: () => call<AboutInfo>("about"),
  diagnostics: (since: number, target: string | null) => call<LogEntry[]>("diagnostics", { since, target }),
  setFullscreen: (fullscreen: boolean) => call<void>("set_fullscreen", { fullscreen }),
  windowPip: (enter: boolean) => call<void>("window_pip", { enter }),
  palette: (item: ItemRef, kind: ImageKind, tag: string) => call<Palette>("palette", { item, kind, tag }),
  cacheClear: () => call<void>("cache_clear"),

  // admin
  adminOverview: (server: ServerId) => call<AdminOverview>("admin_overview", { server }),
  adminRunTask: (server: ServerId, task: string) => call<void>("admin_run_task", { server, task }),
  adminScanLibrary: (library: ItemRef) => call<void>("admin_scan_library", { library }),
};
