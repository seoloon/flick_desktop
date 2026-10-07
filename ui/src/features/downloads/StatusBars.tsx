// Slim bars at the bottom of the screen: "Offline mode" while no server
// answers, and the progress of what is downloading. Each slides in and out.
import { Download, WifiOff } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import type { DownloadItem } from "@/ipc/app-types";
import { bytesText, etaText, percent, remainingSecs, totalRemainingSecs } from "./format";
import { useOffline } from "./OfflineWatcher";
import { useDownloads } from "./store";

const bar = {
  initial: { opacity: 0, y: 16, height: 0 },
  animate: { opacity: 1, y: 0, height: "auto" },
  exit: { opacity: 0, y: 16, height: 0 },
  transition: { duration: 0.4, ease: [0.32, 0.72, 0, 1] as const },
};

function DownloadBar({ items }: { items: Record<string, DownloadItem> }) {
  const running = Object.values(items)
    .filter((d) => d.state === "active")
    .sort((a, b) => a.createdMs - b.createdMs);
  const pending = Object.values(items).filter((d) => d.state === "active" || d.state === "queued");
  const first = running[0];
  const name = first ? [first.title, first.subtitle].filter(Boolean).join(" · ") : "Starting downloads…";
  const left = pending.length > 1 ? totalRemainingSecs(Object.values(items)) : first ? remainingSecs(first) : null;
  const parts = [
    first?.size ? `${bytesText(first.offset)} / ${bytesText(first.size)}` : null,
    first?.speed ? `${bytesText(first.speed)}/s` : null,
    left != null ? `${etaText(left)} left` : null,
  ].filter(Boolean);
  const pct = first ? percent(first) : 0;
  return (
    <motion.div key="downloading" {...bar} className="overflow-hidden">
      <div className="glass relative flex items-center gap-3 overflow-hidden rounded-2xl px-4 py-2.5 text-sm" role="status">
        <Download className="size-4 shrink-0 text-white/80" />
        <span className="min-w-0 flex-1 truncate text-white/90">
          {name}
          {pending.length > 1 && <span className="text-white/50"> · +{pending.length - 1} more</span>}
        </span>
        <span className="shrink-0 text-white/60 tabular-nums">{parts.join(" · ")}</span>
        <div className="absolute inset-x-0 bottom-0 h-0.5 bg-white/10">
          <div className="h-full bg-white/80 transition-[width] duration-300" style={{ width: `${pct}%` }} />
        </div>
      </div>
    </motion.div>
  );
}

function OfflineBar() {
  return (
    <motion.div key="offline" {...bar} className="overflow-hidden">
      <div className="glass flex items-center justify-center gap-2 rounded-2xl px-4 py-2 text-sm text-white/85" role="status">
        <WifiOff className="size-4 shrink-0" />
        Offline mode
        <span className="text-white/50">· only downloaded titles are shown</span>
      </div>
    </motion.div>
  );
}

export function StatusBars() {
  const offline = useOffline((s) => s.offline);
  const items = useDownloads((s) => s.items);
  const busy = Object.values(items).some((d) => d.state === "active" || d.state === "queued");
  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-0 z-40 flex flex-col gap-2 pr-4 pb-3 pl-[calc(var(--content-left)+1rem)]">
      <AnimatePresence initial={false}>
        {busy && <DownloadBar key="d" items={items} />}
        {offline && <OfflineBar key="o" />}
      </AnimatePresence>
    </div>
  );
}
