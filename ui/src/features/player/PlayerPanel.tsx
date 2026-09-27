// The player's Info / Audio / Subtitles panel. It sits over live video, which
// the WebView cannot see (native layer underneath), so it is a dark
// translucent pane rather than real frosted glass.
import { useQuery } from "@tanstack/react-query";
import { Check } from "lucide-react";
import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { Button } from "@/components/tv/Button";
import { Facts, Pill } from "@/components/tv/Page";
import { Segmented } from "@/components/tv/Segmented";
import { api } from "@/ipc/api";
import type { LiveStats } from "@/ipc/bindings/LiveStats";
import type { Track } from "@/ipc/bindings/Track";
import type { TrackType } from "@/ipc/bindings/TrackType";
import { bitrate, channelsLabel } from "@/lib/format";
import { focusSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { summary } from "./explain";
import type { PlayerStore } from "./store";

export type PanelTab = "info" | TrackType;

type Params = Record<string, unknown> | null | undefined;
const field = (n: unknown, k: string) => (n && typeof n === "object" ? (n as Record<string, unknown>)[k] : undefined);
const str = (v: unknown) => (v === undefined || v === null ? "—" : String(v));
const videoLine = (p: Params) =>
  p ? `${str(field(p, "w"))}×${str(field(p, "h"))} ${str(field(p, "hw-pixelformat") ?? field(p, "pixelformat"))} · ${str(field(p, "primaries"))} / ${str(field(p, "gamma"))}` : "—";
const audioLine = (p: Params) => (p ? `${str(field(p, "format"))} · ${str(field(p, "hr-channels"))} · ${str(field(p, "samplerate"))} Hz` : "—");

function trackLabel(t: Track): { title: string; detail: string } {
  const detail = [t.codec?.toUpperCase(), t.kind === "audio" && t.channels ? channelsLabel(t.channels) : "", t.kind === "video" && t.width ? `${t.width}×${t.height}` : "", t.forced ? "Forced" : "", t.externalUrl ? "External" : ""]
    .filter(Boolean)
    .join(" · ");
  const lang = t.language ? new Intl.DisplayNames(undefined, { type: "language" }).of(t.language) ?? t.language.toUpperCase() : null;
  return { title: [lang, t.title].filter(Boolean).join(" — ") || `Track ${t.mpvId}`, detail };
}

function TrackRow({ title, detail, selected, onSelect }: { title: string; detail?: string; selected: boolean; onSelect: () => void }) {
  const tv = useTv<HTMLButtonElement>({ autoFocus: selected });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      role="menuitemradio"
      aria-checked={selected}
      onClick={onSelect}
      animate={{ scale: tv.showFocus ? 1.02 : 1 }}
      transition={focusSpring}
      className={cn(
        "flex w-full cursor-pointer items-center gap-2.5 rounded-lg px-3 py-2 text-left transition-colors",
        tv.showFocus ? "bg-white text-black" : "hover:bg-white/10",
      )}
    >
      <span className="grid size-4 shrink-0 place-items-center">{selected && <Check className="size-3.5" strokeWidth={3} />}</span>
      <span className="flex min-w-0 flex-col">
        <span className="truncate text-[0.8125rem] font-medium">{title}</span>
        {detail && <span className={cn("truncate text-[0.6875rem]", tv.showFocus ? "text-black/60" : "text-white/55")}>{detail}</span>}
      </span>
    </motion.button>
  );
}

function Tracks({ kind, tracks }: { kind: TrackType; tracks: Track[] }) {
  const list = tracks.filter((t) => t.kind === kind);
  const select = (mpvId: number | null) => void api.playerCommand({ type: "selectTrack", kind, mpvId });
  return (
    <FocusGroup fade="y" className="[--fade-size:1.25rem] no-scrollbar -m-1 flex max-h-[42vh] flex-col gap-0.5 overflow-y-auto p-1">
      {kind === "sub" && <TrackRow title="Off" selected={!list.some((t) => t.selected)} onSelect={() => select(null)} />}
      {list.map((t) => {
        const l = trackLabel(t);
        return <TrackRow key={t.mpvId} title={l.title} detail={l.detail} selected={t.selected} onSelect={() => select(t.mpvId)} />;
      })}
      {list.length === 0 && kind !== "sub" && <p className="px-3 py-2 text-white/55">No tracks.</p>}
    </FocusGroup>
  );
}

function InfoTab({ store }: { store: PlayerStore }) {
  const decision = store.state((s) => s.decision);
  const [mode, setMode] = useState<"simple" | "advanced">("simple");
  const [stats, setStats] = useState<LiveStats | null>(null);
  const snapshot = useQuery({ queryKey: ["player-snapshot"], queryFn: () => api.playerSnapshot(), enabled: mode === "advanced" });
  useEffect(() => {
    const pull = () => void api.playerStats().then(setStats, () => undefined);
    pull();
    const t = window.setInterval(pull, 1000);
    return () => window.clearInterval(t);
  }, []);

  if (!decision) return <p className="text-white/60">Negotiating with the server…</p>;
  const s = summary(decision);
  const decoding = stats?.hwdec && stats.hwdec !== "no" ? `Graphics card (${stats.hwdec})` : stats?.hwdec === "no" ? "Processor (software)" : decision.hardwareDecode ? "Graphics card" : "—";
  const notes = decision.reasons.filter((r) => r.severity !== "info");

  const advanced = mode === "advanced";
  return (
    <FocusGroup fade="y" className="[--fade-size:1.25rem] no-scrollbar flex max-h-[46vh] flex-col gap-3 overflow-y-auto">
      <div className="flex flex-col gap-1.5">
        <Pill tone="strong">{s.strategy.title}</Pill>
        <p className="text-white/70">{s.strategy.body}</p>
      </div>
      <Facts
        rows={[
          ["Picture", s.video],
          ["Sound", s.audio],
          ["Decoding", decoding],
        ]}
      />
      {!advanced && notes.length > 0 && (
        <ul className="flex list-disc flex-col gap-1 pl-4 text-amber-100/90">
          {notes.map((r, i) => (
            <li key={i}>{r.message}</li>
          ))}
        </ul>
      )}
      {advanced && (
        <>
          <h3 className="pt-1 text-[0.6875rem] font-semibold tracking-wide text-white/50 uppercase">Measured now</h3>
          <Facts
            rows={[
              ["Video in", videoLine(stats?.videoParams as Params)],
              ["Video out", videoLine(stats?.videoTarget as Params)],
              ["Audio in", audioLine(stats?.audioParams as Params)],
              ["Audio out", `${audioLine(stats?.audioOut as Params)} ${stats?.currentAo ?? ""}`],
              ["Frames", `${str(stats?.containerFps?.toFixed(3))} fps · display ${str(stats?.displayFps?.toFixed(2))} Hz · dropped ${str(stats?.droppedFrames)} / decoder ${str(stats?.decoderDroppedFrames)}`],
              ["A/V sync", stats?.avsync != null ? `${(stats.avsync * 1000).toFixed(1)} ms` : "—"],
              ["Bitrate", `video ${bitrate(stats?.videoBitrate) || "—"} · audio ${bitrate(stats?.audioBitrate) || "—"}`],
              ["Presenter", snapshot.data?.presenter ?? "—"],
            ]}
          />
          <h3 className="pt-1 text-[0.6875rem] font-semibold tracking-wide text-white/50 uppercase">Decision steps</h3>
          <ul className="flex flex-col gap-1">
            {decision.reasons.map((r, i) => (
              <li key={i} className={cn(r.severity === "degraded" ? "text-amber-100" : r.severity === "blocking" ? "text-red-200" : "text-white/75")}>
                <code className="mr-1.5 rounded bg-white/10 px-1 py-0.5 font-mono text-[0.6875rem]">{r.code}</code>
                {r.message}
              </li>
            ))}
          </ul>
          <h3 className="pt-1 text-[0.6875rem] font-semibold tracking-wide text-white/50 uppercase">Applied engine options</h3>
          <Facts mono rows={snapshot.data?.appliedOptions ?? []} />
        </>
      )}
      <div>
        <Button variant="ghost" size="sm" onClick={() => setMode(advanced ? "simple" : "advanced")} className="-ml-3">
          {advanced ? "Fewer details" : "Technical details"}
        </Button>
      </div>
    </FocusGroup>
  );
}

export function PlayerPanel({ tab, onTab, store }: { tab: PanelTab; onTab: (t: PanelTab) => void; store: PlayerStore }) {
  const tracks = store.state((s) => s.tracks);
  const hasVideoChoice = tracks.filter((t) => t.kind === "video").length > 1;
  const tabs: { value: PanelTab; label: string }[] = [
    { value: "info", label: "Info" },
    { value: "audio", label: "Audio" },
    { value: "sub", label: "Subtitles" },
    ...(hasVideoChoice ? [{ value: "video" as const, label: "Video" }] : []),
  ];
  return (
    <motion.aside
      aria-label="Playback options"
      initial={{ opacity: 0, y: 16, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 12, scale: 0.98 }}
      transition={{ type: "spring", stiffness: 300, damping: 32 }}
      className="absolute right-8 bottom-[calc(100%+0.25rem)] w-[min(28rem,calc(100vw-4rem))] rounded-2xl bg-black/75 p-3.5 text-[0.8125rem] text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.12),inset_0_0_0_1px_rgb(255_255_255/0.08),0_30px_80px_-20px_rgb(0_0_0/0.8)]"
    >
      <FocusGroup focusKey="player-panel" boundary autoFocus className="flex flex-col gap-3">
        <Segmented size="sm" options={tabs} value={tab} onChange={onTab} label="Panel" />
        {tab === "info" ? <InfoTab store={store} /> : <Tracks key={tab} kind={tab} tracks={tracks} />}
      </FocusGroup>
    </motion.aside>
  );
}
