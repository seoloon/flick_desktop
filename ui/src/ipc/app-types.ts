// DTOs defined in the Tauri shell crate (`app/src/commands`). Domain types
// are generated from Rust into ./bindings by ts-rs; keep these in sync with
// the shell's `#[derive(Serialize)]` structs.
import type { AdminServerInfo } from "./bindings/AdminServerInfo";
import type { AdminSession } from "./bindings/AdminSession";
import type { AdminTask } from "./bindings/AdminTask";
import type { AdminUser } from "./bindings/AdminUser";
import type { ServerDescriptor } from "./bindings/ServerDescriptor";
import type { AvatarStyle } from "./bindings/AvatarStyle";
import type { ServerId } from "./bindings/ServerId";
import type { UserMessage } from "./bindings/UserMessage";

export type AppError = { kind: string; message: string };
/** serde's default encoding of `Result<T, E>`. */
export type RustResult<T, E = AppError> = { Ok: T } | { Err: E };

export type ServerEntry = { server: ServerDescriptor; connected: boolean };
export type ProbeResult = { url: string; name: string; version: string | null };
export type QuickConnect = { code: string; secret: string };
export type PlexPin = { id: number; code: string; authUrl: string };
export type PlexServerChoice = {
  machineId: string;
  name: string;
  owned: boolean;
  version: string | null;
  url: string | null;
};
export type AboutInfo = {
  version: string;
  libmpv: RustResult<[string, [number, number]], string>;
  credentialStore: boolean;
  configDir: string;
  cacheDir: string;
  display: string | null;
};
/** A newer release than the running one (app/src/commands/updates.rs). */
export type UpdateInfo = {
  version: string;
  currentVersion: string;
  /** Markdown, from the GitHub release. */
  notes: string | null;
  /** RFC 3339. */
  date: string | null;
};
export type InstallProgress = { event: "started"; total: number | null } | { event: "progress"; downloaded: number } | { event: "installing" };
export type LogEntry = {
  seq: number;
  time: string;
  level: string;
  target: string;
  message: string;
  fields: string;
};
export type Palette = { colors: string[]; base: string; accent: string };
export type AdminOverview = {
  info: RustResult<AdminServerInfo>;
  sessions: RustResult<AdminSession[]>;
  tasks: RustResult<AdminTask[]>;
  users: RustResult<AdminUser[]>;
  logs: RustResult<string[]>;
};
export type CssRect = { x: number; y: number; width: number; height: number };

/** Offline downloads (app/src/downloads.rs, mirrors `oneshot_flickdd::Item`). */
export type DownloadState = "queued" | "active" | "paused" | "done" | "failed";
export type DownloadItem = {
  id: string;
  backend: "jellyfin" | "plex";
  itemId: string;
  /** The library item this download comes from (an `ItemRef`). */
  itemRef: string;
  title: string;
  subtitle: string | null;
  kind: "movie" | "episode" | null;
  state: DownloadState;
  /** Bytes safely on disk. */
  offset: number;
  size: number | null;
  filename: string | null;
  finalPath: string | null;
  /** Why it is paused or failed. */
  error: string | null;
  /** What it is doing when that is not downloading (waiting, retrying). */
  note: string | null;
  /** Bytes per second. */
  speed: number | null;
  /** Finished, but the file is gone. */
  missing: boolean;
  createdMs: number;
};
export type DownloadEvent = { type: "changed"; item: DownloadItem } | { type: "removed"; id: string };
export type DownloadsStatus = {
  /** A Flick Server invitation link is saved. */
  configured: boolean;
  /** Where downloads are written (the app's own data folder). */
  directory: string;
  /** The library finished downloads belong to: `<localServer>:<download id>` is a title. */
  localServer: string;
};

/** Watch together (app/src/flicksync.rs). */
export type FlickSyncStatus = {
  /** Configured and reachable: rooms can be offered. */
  available: boolean;
  configured: boolean;
  inRoom: boolean;
  message: UserMessage | null;
};

/** One step of "Test connection" (mirrors `oneshot_flicksync::diagnose`). */
export type DiagnosisCheck = {
  step: "config" | "reach" | "ready" | "clock" | "auth";
  status: "ok" | "failed" | "skipped";
  detail: string;
};

export type FlickSyncDiagnosis = { checks: DiagnosisCheck[] };

/** The saved invitation as the UI may see it: never the key. */
export type InvitationInfo = {
  /** `host[:port][/prefix]`. */
  address: string;
  tls: boolean;
  /** Plain HTTP across the Internet: tokens would travel in the clear. */
  insecureRemote: boolean;
};

/** Answer to pasting a link. `saved` is false when the server did not answer or was not ready. */
export type InvitationAdded = { info: InvitationInfo; saved: boolean; report: FlickSyncDiagnosis };

export type SwitchOutcome = { failed: string[] };
export type ProfileEdit = {
  name: string | null;
  color: string | null;
  avatar: AvatarStyle | null;
  connections: ServerId[] | null;
  hidden: boolean | null;
};
