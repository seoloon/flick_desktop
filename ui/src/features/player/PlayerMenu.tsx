// The player's single settings menu (like Apple's player): a root page with
// Audio, Subtitles, Video and Playback Info, each opening a page that slides
// in. The card resizes smoothly between pages (animate-ui AutoHeight).
//
// It sits over live video, which the WebView cannot see (native layer
// underneath), so it is a dark translucent card rather than frosted glass.
import { useQuery } from "@tanstack/react-query";
import { Check, ChevronLeft, ChevronRight } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type MutableRefObject, type ReactNode, useEffect, useRef, useState } from "react";
import { AutoHeight } from "@/components/animate-ui/primitives/effects/auto-height";
import { Button } from "@/components/tv/Button";
import { Facts, Pill } from "@/components/tv/Page";
import { api } from "@/ipc/api";
import type { LiveStats } from "@/ipc/bindings/LiveStats";
import type { Track } from "@/ipc/bindings/Track";
import type { TrackType } from "@/ipc/bindings/TrackType";
import { bitrate, channelsLabel } from "@/lib/format";
import { focusSpring, panelSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import type { Action } from "@/nav/input";
import { focusKey } from "@/nav/spatial";
import { summary } from "./explain";
import type { PlayerStore } from "./store";

type Page = "root" | "info" | TrackType;

/** Lets the player route Back/Left to the menu before closing it. */
export type MenuActions = MutableRefObject<((a: Action) => boolean) | null>;

type Params = Record<string, unknown> | null | undefined;
const field = (n: unknown, k: string) => (n && typeof n === "object" ? (n as Record<string, unknown>)[k] : undefined);
const str = (v: unknown) => (v === undefined || v === null ? "—" : String(v));
const videoLine = (p: Params) =>
  p ? `${str(field(p, "w"))}×${str(field(p, "h"))} ${str(field(p, "hw-pixelformat") ?? field(p, "pixelformat"))} · ${str(field(p, "primaries"))} / ${str(field(p, "gamma"))}` : "—";
const audioLine = (p: Params) => (p ? `${str(field(p, "format"))} · ${str(field(p, "hr-channels"))} · ${str(field(p, "samplerate"))} Hz` : "—");

const languageNames = new Intl.DisplayNames(undefined, { type: "language" });

function trackLabel(t: Track): { title: string; detail: string } {
  const detail = [t.codec?.toUpperCase(), t.kind === "audio" && t.channels ? channelsLabel(t.channels) : "", t.kind === "video" && t.width ? `${t.width}×${t.height}` : "", t.forced ? "Forced" : "", t.externalUrl ? "External" : ""]
    .filter(Boolean)
    .join(" · ");
  let lang: string | null = null;
  try {
    lang = t.language ? (languageNames.of(t.language) ?? t.language.toUpperCase()) : null;
  } catch {
    lang = t.language?.toUpperCase() ?? null;
  }
  return { title: [lang, t.title].filter(Boolean).join(" — ") || `Track ${t.mpvId}`, detail };
}

const titles: Record<Page, string> = { root: "", info: "Playback Info", audio: "Audio", sub: "Subtitles", video: "Video" };

/** A row in any page: white on focus, lifts a touch. */
function Row({ focusKey: key, autoFocus, onSelect, children, role, checked }: { focusKey?: string; autoFocus?: boolean; onSelect: () => void; children: ReactNode; role?: string; checked?: boolean }) {
  const tv = useTv<HTMLButtonElement>({ focusKey: key, autoFocus, scroll: "nearest" });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      role={role}
      aria-checked={checked}
      onClick={onSelect}
      data-tv-focus={tv.showFocus || undefined}
      animate={{ scale: tv.showFocus ? 1.02 : 1 }}
      transition={focusSpring}
      className={cn(
        "group/row flex w-full cursor-pointer items-center gap-2.5 rounded-lg px-3 py-2 text-left transition-colors duration-200",
        tv.showFocus ? "bg-white text-black" : "hover:bg-white/10",
      )}
    >
      {children}
    </motion.button>
  );
}

function Tracks({ kind, tracks }: { kind: TrackType; tracks: Track[] }) {
  const list = tracks.filter((t) => t.kind === kind);
  const select = (mpvId: number | null) => void api.playerCommand({ type: "selectTrack", kind, mpvId });
  const none = !list.some((t) => t.selected);
  const item = (key: string, title: string, detail: string, selected: boolean, onSelect: () => void) => (
    <Row key={key} role="menuitemradio" checked={selected} autoFocus={selected} onSelect={onSelect}>
      <span className="grid size-4 shrink-0 place-items-center">
        <AnimatePresence>{selected && <motion.span initial={{ scale: 0 }} animate={{ scale: 1 }} exit={{ scale: 0 }} transition={focusSpring}><Check className="size-3.5" strokeWidth={3} /></motion.span>}</AnimatePresence>
      </span>
      <span className="flex min-w-0 flex-col">
        <span className="truncate text-[0.8125rem] font-medium">{title}</span>
        {detail && <span className="truncate text-[0.6875rem] text-white/55 group-data-tv-focus/row:text-black/55">{detail}</span>}
      </span>
    </Row>
  );
  return (
    <FocusGroup fade="y" className="[--fade-size:1.25rem] no-scrollbar -m-1 flex max-h-[40vh] flex-col gap-0.5 overflow-y-auto p-1">
      {kind === "sub" && item("off", "Off", "", none, () => select(null))}
      {list.map((t) => {
        const l = trackLabel(t);
        return item(String(t.mpvId), l.title, l.detail, t.selected, () => select(t.mpvId));
      })}
      {list.length === 0 && kind !== "sub" && <p className="px-3 py-2 text-white/55">No tracks.</p>}
    </FocusGroup>
  );
}

function Info({ store }: { store: PlayerStore }) {
  const decision = store.state((s) => s.decision);
  const [advanced, setAdvanced] = useState(false);
  const [stats, setStats] = useState<LiveStats | null>(null);
  const snapshot = useQuery({ queryKey: ["player-snapshot"], queryFn: () => api.playerSnapshot(), enabled: advanced });
  useEffect(() => {
    const pull = () => void api.playerStats().then(setStats, () => undefined);
    pull();
    const t = window.setInterval(pull, 1000);
    return () => window.clearInterval(t);
  }, []);

  if (!decision) return <p className="px-3 py-2 text-white/60">Negotiating with the server…</p>;
  const s = summary(decision);
  const decoding = stats?.hwdec && stats.hwdec !== "no" ? `Graphics card (${stats.hwdec})` : stats?.hwdec === "no" ? "Processor (software)" : decision.hardwareDecode ? "Graphics card" : "—";
  const notes = decision.reasons.filter((r) => r.severity !== "info");
  const heading = "pt-1 text-[0.6875rem] font-semibold tracking-wide text-white/50 uppercase";

  return (
    <FocusGroup fade="y" className="[--fade-size:1.25rem] no-scrollbar flex max-h-[46vh] flex-col gap-3 overflow-y-auto px-3 pb-1">
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
        <motion.div initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.3 }} className="flex flex-col gap-3">
          <h3 className={heading}>Measured now</h3>
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
          <h3 className={heading}>Decision steps</h3>
          <ul className="flex flex-col gap-1">
            {decision.reasons.map((r, i) => (
              <li key={i} className={cn(r.severity === "degraded" ? "text-amber-100" : r.severity === "blocking" ? "text-red-200" : "text-white/75")}>
                <code className="mr-1.5 rounded bg-white/10 px-1 py-0.5 font-mono text-[0.6875rem]">{r.code}</code>
                {r.message}
              </li>
            ))}
          </ul>
          <h3 className={heading}>Applied engine options</h3>
          <Facts mono rows={snapshot.data?.appliedOptions ?? []} />
        </motion.div>
      )}
      <div>
        <Button variant="ghost" size="sm" onClick={() => setAdvanced((a) => !a)} className="-ml-4">
          {advanced ? "Fewer Details" : "Technical Details"}
        </Button>
      </div>
    </FocusGroup>
  );
}

const slide = {
  enter: (dir: number) => ({ x: dir * 48, opacity: 0, filter: "blur(4px)" }),
  center: { x: 0, opacity: 1, filter: "blur(0px)" },
  exit: (dir: number) => ({ x: dir * -48, opacity: 0, filter: "blur(4px)" }),
};

export function PlayerMenu({ store, actions }: { store: PlayerStore; actions: MenuActions }) {
  const tracks = store.state((s) => s.tracks);
  const decision = store.state((s) => s.decision);
  const [page, setPage] = useState<Page>("root");
  const [dir, setDir] = useState(1);
  const cameFrom = useRef<Page | null>(null);

  const open = (p: Page) => {
    cameFrom.current = page;
    setDir(p === "root" ? -1 : 1);
    setPage(p);
  };

  // Back or Left on a sub-page returns to the root, on the row it came from.
  actions.current = (a) => {
    if (page === "root") return false;
    if (a.type === "back" || (a.type === "move" && a.dir === "left")) {
      open("root");
      return true;
    }
    return false;
  };
  useEffect(() => () => void (actions.current = null), [actions]);
  useEffect(() => {
    if (page !== "root" || !cameFrom.current || cameFrom.current === "root") return;
    const key = `menu:${cameFrom.current}`;
    requestAnimationFrame(() => focusKey(key));
  }, [page]);

  const selected = (kind: TrackType) => tracks.find((t) => t.kind === kind && t.selected);
  const audio = selected("audio");
  const sub = selected("sub");
  const rows: { page: Page; label: string; value: string }[] = [
    { page: "audio", label: "Audio", value: audio ? trackLabel(audio).title : "—" },
    { page: "sub", label: "Subtitles", value: sub ? trackLabel(sub).title : "Off" },
    ...(tracks.filter((t) => t.kind === "video").length > 1 ? [{ page: "video" as const, label: "Video", value: trackLabel(selected("video") ?? tracks.find((t) => t.kind === "video")!).detail }] : []),
    { page: "info", label: "Playback Info", value: decision ? summary(decision).strategy.title : "" },
  ];

  return (
    <motion.aside
      aria-label="Playback settings"
      initial={{ opacity: 0, y: 12, scale: 0.94 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 8, scale: 0.96 }}
      transition={panelSpring}
      style={{ transformOrigin: "bottom right" }}
      className="absolute right-8 bottom-[calc(100%+0.25rem)] w-[min(24rem,calc(100vw-4rem))] overflow-hidden rounded-2xl bg-black/75 p-1.5 text-[0.8125rem] text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.12),inset_0_0_0_1px_rgb(255_255_255/0.08),0_30px_80px_-20px_rgb(0_0_0/0.8)]"
    >
      <FocusGroup focusKey="player-menu" boundary autoFocus>
        <AutoHeight deps={[page]} transition={panelSpring}>
          <div className="relative">
            <AnimatePresence mode="popLayout" initial={false} custom={dir}>
              <motion.div key={page} custom={dir} variants={slide} initial="enter" animate="center" exit="exit" transition={panelSpring} className="flex flex-col gap-0.5">
                {page === "root" ? (
                  rows.map((r) => (
                    <Row key={r.page} focusKey={`menu:${r.page}`} onSelect={() => open(r.page)}>
                      <span className="shrink-0 font-medium">{r.label}</span>
                      <span className="ml-auto min-w-0 truncate text-right text-white/55 group-data-tv-focus/row:text-black/55">{r.value}</span>
                      <ChevronRight className="size-4 shrink-0 opacity-50" />
                    </Row>
                  ))
                ) : (
                  <>
                    <Row onSelect={() => open("root")}>
                      <ChevronLeft className="size-4 shrink-0 opacity-60" />
                      <span className="font-semibold">{titles[page]}</span>
                    </Row>
                    <div className="mx-3 my-1 h-px bg-white/10" />
                    {page === "info" ? <Info store={store} /> : <Tracks kind={page} tracks={tracks} />}
                  </>
                )}
              </motion.div>
            </AnimatePresence>
          </div>
        </AutoHeight>
      </FocusGroup>
    </motion.aside>
  );
}
