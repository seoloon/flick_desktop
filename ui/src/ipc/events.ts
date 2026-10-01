import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ClientEvent } from "./bindings/ClientEvent";
import type { PlayerEvent } from "./bindings/PlayerEvent";

export function onPlayerEvent(handler: (e: PlayerEvent) => void): Promise<UnlistenFn> {
  return listen<PlayerEvent>("player", (event) => handler(event.payload));
}

/** Room events, plus the app's request to open the player for the room's title. */
export type FlickSyncEvent = ClientEvent | { type: "openPlayer"; item: string };

export function onFlickSyncEvent(handler: (e: FlickSyncEvent) => void): Promise<UnlistenFn> {
  return listen<FlickSyncEvent>("flicksync", (event) => handler(event.payload));
}
