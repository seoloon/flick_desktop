// DTOs defined in the Tauri shell crate (`app/src/commands`). Domain types
// are generated from Rust into ./bindings by ts-rs; keep these in sync with
// the shell's `#[derive(Serialize)]` structs.
import type { AdminServerInfo } from "./bindings/AdminServerInfo";
import type { AdminSession } from "./bindings/AdminSession";
import type { AdminTask } from "./bindings/AdminTask";
import type { AdminUser } from "./bindings/AdminUser";
import type { ServerDescriptor } from "./bindings/ServerDescriptor";

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
