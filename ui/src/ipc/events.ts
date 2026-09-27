import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { PlayerEvent } from "./bindings/PlayerEvent";

export function onPlayerEvent(handler: (e: PlayerEvent) => void): Promise<UnlistenFn> {
  return listen<PlayerEvent>("player", (event) => handler(event.payload));
}
