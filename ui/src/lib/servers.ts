// Which server an item comes from. An ItemRef is "<server-id>:<provider-key>"
// and playback always uses the item's own reference, so its server is the
// source; `alternates` are the same title on other servers.
import { useQuery } from "@tanstack/react-query";
import type { ItemRef } from "@/ipc/bindings/ItemRef";
import type { ServerDescriptor } from "@/ipc/bindings/ServerDescriptor";
import { useDownloads } from "@/features/downloads/store";
import { serversQuery } from "@/shell/navItems";

export function serverIdOf(ref: ItemRef): string {
  const i = ref.indexOf(":");
  return i < 0 ? ref : ref.slice(0, i);
}

/**
 * Resolves the servers of `refs`, or nothing when a single server is
 * configured: the source is only worth showing when there is a choice.
 */
export function useSources(refs: ItemRef[]): ServerDescriptor[] {
  const servers = useQuery(serversQuery).data?.filter((s) => s.connected && !s.server.disabled);
  if (!servers || servers.length < 2) return [];
  const byId = new Map(servers.map((s) => [s.server.id, s.server]));
  const out: ServerDescriptor[] = [];
  for (const ref of refs) {
    const s = byId.get(serverIdOf(ref));
    if (s && !out.includes(s)) out.push(s);
  }
  return out;
}

/** The item is a download kept on this computer (its library is the local one). */
export function useIsLocal(ref: ItemRef | undefined): boolean {
  const local = useDownloads((d) => d.status?.localServer);
  return !!ref && !!local && serverIdOf(ref) === local;
}
