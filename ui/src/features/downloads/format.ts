// What a download looks like to the user.
import type { DownloadItem } from "@/ipc/app-types";

const UNITS = ["B", "KB", "MB", "GB", "TB"];

/** 1536 -> "1.5 KB"; one decimal below 100, none above. */
export function bytesText(n: number): string {
  let v = Math.max(0, n);
  let u = 0;
  while (v >= 1024 && u < UNITS.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v >= 100 || u === 0 ? Math.round(v) : v.toFixed(1)} ${UNITS[u]}`;
}

/** "45 s", "12 min", "1 h 05". Whole seconds in, rounded up to what is worth reading. */
export function etaText(secs: number): string {
  const s = Math.max(1, Math.ceil(secs));
  if (s < 60) return `${s} s`;
  const m = Math.ceil(s / 60);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, "0")}`;
}

/** Time left for one download, when it is moving and its size is known. */
export function remainingSecs(d: Pick<DownloadItem, "offset" | "size" | "speed" | "state">): number | null {
  if (d.state !== "active" || !d.size || !d.speed) return null;
  return Math.max(0, d.size - d.offset) / d.speed;
}

/**
 * Time left for everything in the queue: the bytes still to fetch, at the speed the running downloads
 * add up to. Null while nothing is moving or a size is still unknown.
 */
export function totalRemainingSecs(items: DownloadItem[]): number | null {
  const pending = items.filter((d) => d.state === "active" || d.state === "queued");
  const speed = items.reduce((sum, d) => sum + (d.state === "active" ? (d.speed ?? 0) : 0), 0);
  if (pending.length === 0 || speed === 0 || pending.some((d) => !d.size)) return null;
  return pending.reduce((sum, d) => sum + Math.max(0, (d.size ?? 0) - d.offset), 0) / speed;
}

/** Share of the file on disk, 0 to 100. */
export function percent(d: Pick<DownloadItem, "offset" | "size" | "state">): number {
  if (d.state === "done") return 100;
  if (!d.size) return 0;
  return Math.min(100, Math.floor((d.offset / d.size) * 100));
}

/** The line under a download's title. */
export function stateText(d: DownloadItem): string {
  switch (d.state) {
    case "queued":
      return "Waiting to start";
    case "active": {
      if (d.note) return d.note;
      const done = d.size ? `${bytesText(d.offset)} of ${bytesText(d.size)}` : "Starting…";
      const left = remainingSecs(d);
      return d.speed ? `${done} · ${bytesText(d.speed)}/s${left != null ? ` · ${etaText(left)} left` : ""}` : done;
    }
    case "paused":
      return d.error ?? `Paused · ${percent(d)} %`;
    case "failed":
      return d.error ?? "Could not be downloaded";
    case "done":
      return d.missing ? "The file is no longer there" : d.size ? `Downloaded · ${bytesText(d.size)}` : "Downloaded";
  }
}

/** Sort: running first, then waiting, paused, failed, finished (newest first inside each). */
export function sortDownloads(items: DownloadItem[]): DownloadItem[] {
  const rank = { active: 0, queued: 1, paused: 2, failed: 3, done: 4 } as const;
  return [...items].sort((a, b) => rank[a.state] - rank[b.state] || b.createdMs - a.createdMs);
}
