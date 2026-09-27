// Diagnostics: structured logs by area, the machine's capability report and
// the last playback's decision/applied options.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/tv/Button";
import { Page, PageHeader } from "@/components/tv/Page";
import { Segmented } from "@/components/tv/Segmented";
import { api } from "@/ipc/api";
import type { LogEntry } from "@/ipc/app-types";
import { cn } from "@/lib/utils";
import { Screen } from "@/nav/Focusable";

const areas = [
  { value: "", label: "All" },
  { value: "playback", label: "Decisions" },
  { value: "player", label: "Player" },
  { value: "mpv", label: "Engine" },
  { value: "provider", label: "Servers" },
  { value: "capabilities", label: "Capabilities" },
  { value: "catalog", label: "Library" },
  { value: "cache", label: "Cache" },
];

const levelClass: Record<string, string> = {
  ERROR: "text-red-300",
  WARN: "text-amber-200",
  DEBUG: "text-white/45",
  TRACE: "text-white/30",
};

function Logs({ area }: { area: string }) {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const since = useRef(0);
  useEffect(() => {
    const pull = async () => {
      const fresh = await api.diagnostics(since.current, null).catch(() => []);
      if (fresh.length) {
        since.current = fresh[fresh.length - 1]!.seq;
        setLogs((l) => [...l, ...fresh].slice(-2000));
      }
    };
    void pull();
    const t = window.setInterval(pull, 1000);
    return () => window.clearInterval(t);
  }, []);
  const shown = logs
    .filter((l) => !area || l.target.startsWith(area))
    .slice(-500)
    .reverse();
  return (
    <div className="glass max-h-[65vh] overflow-auto rounded-2xl p-4 font-mono text-xs leading-relaxed select-text">
      {shown.map((l) => (
        <div key={l.seq} className={cn("grid grid-cols-[6.5rem_9rem_1fr] gap-3 py-0.5", levelClass[l.level.toUpperCase()] ?? "text-white/85")}>
          <span className="text-white/40">{l.time.slice(11, 23)}</span>
          <span className="truncate text-white/55">{l.target}</span>
          <span className="break-words">
            {l.message} <span className="text-white/40">{l.fields}</span>
          </span>
        </div>
      ))}
      {shown.length === 0 && <p className="text-muted-foreground">No entries yet.</p>}
    </div>
  );
}

function Json({ value }: { value: unknown }) {
  return <pre className="glass max-h-[65vh] overflow-auto rounded-2xl p-5 font-mono text-xs leading-relaxed text-white/85 select-text">{JSON.stringify(value, null, 2)}</pre>;
}

export function Debug() {
  const queryClient = useQueryClient();
  const [view, setView] = useState<"logs" | "caps" | "player">("logs");
  const [area, setArea] = useState("");
  const caps = useQuery({ queryKey: ["capabilities"], queryFn: () => api.capabilities(false), enabled: view === "caps" });
  const snapshot = useQuery({ queryKey: ["player-snapshot"], queryFn: () => api.playerSnapshot(), enabled: view === "player" });

  return (
    <Screen>
      <Page>
        <PageHeader
          title="Diagnostics"
          actions={
            <Segmented
              options={[
                { value: "logs", label: "Logs" },
                { value: "caps", label: "This Computer" },
                { value: "player", label: "Last Playback" },
              ]}
              value={view}
              onChange={setView}
            />
          }
        />
        {view === "logs" && (
          <>
            <Segmented size="sm" options={areas} value={area} onChange={setArea} label="Area" />
            <Logs area={area} />
          </>
        )}
        {view === "caps" && (
          <>
            <div>
              <Button icon={RefreshCw} onClick={() => void api.capabilities(true).then((c) => queryClient.setQueryData(["capabilities"], c))}>
                Probe Again
              </Button>
            </div>
            <Json value={caps.data ?? null} />
          </>
        )}
        {view === "player" && <Json value={snapshot.data ?? null} />}
      </Page>
    </Screen>
  );
}
