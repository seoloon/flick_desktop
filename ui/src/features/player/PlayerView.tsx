// Full-window player. The page is transparent: the native video layer is
// composited behind it by the OS (see ARCHITECTURE.md §4). This component only
// draws chrome and forwards intents to Rust.
//
// Remote model (tvOS): the scrubber has focus by default, left/right skip
// 10 s, Enter pauses, Up opens the Info panel, Down reaches the buttons. With
// the controls hidden, any key reveals them; left/right also skip at once.
import { useQuery } from "@tanstack/react-query";
import {
  ChevronLeft,
  Info,
  Languages,
  Maximize,
  Minimize,
  Pause,
  Play,
  RotateCcw,
  RotateCw,
  SkipBack,
  SkipForward,
  Subtitles,
  Volume2,
  VolumeX,
  type LucideIcon,
} from "lucide-react";
import { AnimatePresence, motion, type MotionValue, useMotionValueEvent, useTransform } from "motion/react";
import { type CSSProperties, type PointerEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { SlidingNumber } from "@/components/animate-ui/primitives/texts/sliding-number";
import { Button } from "@/components/tv/Button";
import { Spinner } from "@/components/tv/Feedback";
import { api, asError } from "@/ipc/api";
import type { Marker } from "@/ipc/bindings/Marker";
import type { MediaItem } from "@/ipc/bindings/MediaItem";
import type { PlayerCommand } from "@/ipc/bindings/PlayerCommand";
import { onPlayerEvent } from "@/ipc/events";
import { imageUrl } from "@/ipc/images";
import { clock, episodeLabel } from "@/lib/format";
import { focusSpring } from "@/lib/motion";
import { toggleFrame, useMode } from "@/lib/mode";
import { useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { onAction } from "@/nav/input";
import { focusKey } from "@/nav/spatial";
import { TitleBar } from "@/shell/TitleBar";
import { type PanelTab, PlayerPanel } from "./PlayerPanel";
import { playPath } from "./route";
import { createPlayerStore, type PlayerStore } from "./store";

const HIDE_AFTER = 3500;
const NEXT_UP_WINDOW = 30_000;
const SEEK_STEP = 10_000;
const TIMELINE_KEY = "player-timeline";

const cmd = (c: PlayerCommand) => void api.playerCommand(c).catch(() => undefined);

/** Re-renders once per second of playback, for logic that needs the position. */
function useSecond(position: MotionValue<number>) {
  const [sec, setSec] = useState(0);
  useMotionValueEvent(position, "change", (v) => {
    const s = Math.floor(v / 1000);
    setSec((prev) => (prev === s ? prev : s));
  });
  return sec * 1000;
}

function Clock({ value, remainingOf, className }: { value: MotionValue<number>; remainingOf?: number; className?: string }) {
  const ms = useSecond(value);
  const shown = remainingOf != null ? Math.max(0, remainingOf - ms) : ms;
  return (
    <span className={cn("tabular-nums", className)}>
      {remainingOf != null && "−"}
      {clock(shown)}
    </span>
  );
}

function Timeline({ store, markers, onSeek, onScrub }: { store: PlayerStore; markers: Marker[]; onSeek: (ms: number) => void; onScrub: (ms: number | null) => void }) {
  const duration = store.state((s) => s.duration);
  const buffered = store.state((s) => s.buffered);
  const chapters = store.state((s) => s.chapters);
  const tv = useTv<HTMLDivElement>({ focusKey: TIMELINE_KEY, scroll: false });
  const pct = (ms: number) => (duration ? `${Math.min(100, (ms / duration) * 100)}%` : "0%");
  const fill = useTransform(store.position, (p) => pct(p));

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const at = (x: number) => {
      const r = el.getBoundingClientRect();
      return Math.min(1, Math.max(0, (x - r.left) / r.width)) * duration;
    };
    onScrub(at(e.clientX));
    const move = (ev: globalThis.PointerEvent) => onScrub(at(ev.clientX));
    const up = (ev: globalThis.PointerEvent) => {
      onSeek(at(ev.clientX));
      onScrub(null);
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  };

  return (
    <div
      ref={tv.ref}
      {...tv.props}
      tabIndex={0}
      role="slider"
      aria-label="Position"
      aria-valuemin={0}
      aria-valuemax={duration}
      onPointerDown={onPointerDown}
      onClick={(e) => e.detail === 0 && cmd({ type: "togglePause" })}
      className="group/timeline relative flex h-5 min-w-0 flex-1 cursor-pointer items-center"
    >
      <motion.div
        className="relative h-1 w-full overflow-hidden rounded-full bg-white/25"
        animate={{ height: tv.showFocus ? 6 : 4 }}
        whileHover={{ height: 6 }}
        transition={focusSpring}
      >
        <div className="absolute inset-y-0 left-0 bg-white/30" style={{ width: pct(buffered) }} />
        {markers.map((m, i) => (
          <span key={i} className="absolute inset-y-0 bg-white/20" style={{ left: pct(m.startMs), width: pct(m.endMs - m.startMs) }} />
        ))}
        <motion.div className="absolute inset-y-0 left-0 rounded-full bg-white" style={{ width: fill }} />
        {chapters.map((c, i) => (
          <span key={i} className="absolute inset-y-0 w-0.5 bg-black/50" style={{ left: pct(c.startMs) }} />
        ))}
      </motion.div>
      <motion.span
        className="pointer-events-none absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white shadow-[0_2px_10px_rgb(0_0_0/0.5)]"
        style={{ left: fill }}
        animate={{ scale: tv.showFocus ? 1.25 : 0.001, opacity: tv.showFocus ? 1 : 0 }}
        transition={focusSpring}
      />
    </div>
  );
}

function VolumeControl({ store }: { store: PlayerStore }) {
  const volume = store.state((s) => s.volume);
  const muted = store.state((s) => s.muted);
  const tv = useTv<HTMLDivElement>({ scroll: false });
  useEffect(() => {
    if (!tv.focused) return;
    return onAction((a) => {
      if (a.type !== "move" || (a.dir !== "left" && a.dir !== "right")) return false;
      cmd({ type: "setVolume", volume: Math.max(0, Math.min(100, volume + (a.dir === "left" ? -5 : 5))) });
      return true;
    });
  }, [tv.focused, volume]);
  const shown = muted ? 0 : volume;
  const drag = (e: PointerEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const at = (x: number) => {
      const r = el.getBoundingClientRect();
      return Math.round(Math.min(1, Math.max(0, (x - r.left) / r.width)) * 100);
    };
    cmd({ type: "setVolume", volume: at(e.clientX) });
    const move = (ev: globalThis.PointerEvent) => cmd({ type: "setVolume", volume: at(ev.clientX) });
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  };
  return (
    <div className="flex items-center gap-1">
      <Button variant="ghost" size="icon-sm" icon={muted || volume === 0 ? VolumeX : Volume2} label={muted ? "Unmute" : "Mute"} onClick={() => cmd({ type: "setMute", muted: !muted })} />
      <motion.div
        ref={tv.ref}
        {...tv.props}
        tabIndex={0}
        role="slider"
        aria-label="Volume"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={shown}
        onPointerDown={drag}
        animate={{ scale: tv.showFocus ? 1.08 : 1 }}
        transition={focusSpring}
        className={cn("flex h-9 w-24 cursor-pointer items-center rounded-full px-3 transition-colors", tv.showFocus && "bg-white")}
      >
        <span className={cn("h-1 w-full overflow-hidden rounded-full", tv.showFocus ? "bg-black/15" : "bg-white/25")}>
          <span className={cn("block h-full rounded-full", tv.showFocus ? "bg-black" : "bg-white")} style={{ width: `${shown}%` }} />
        </span>
      </motion.div>
    </div>
  );
}

function NextUp({ next, countdown, onPlay, onDismiss }: { next: MediaItem; countdown: number | null; onPlay: () => void; onDismiss: () => void }) {
  const art = imageUrl(next.images.thumb ?? next.images.backdrop, "card");
  return (
    <motion.div
      initial={{ opacity: 0, x: 40 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 40 }}
      transition={{ type: "spring", stiffness: 260, damping: 30 }}
      className="absolute right-8 bottom-[calc(var(--bar-h)+1rem)] w-[21rem] overflow-hidden rounded-2xl bg-black/75 p-3 text-sm shadow-[inset_0_1px_0_rgb(255_255_255/0.12),inset_0_0_0_1px_rgb(255_255_255/0.08),0_30px_60px_-20px_rgb(0_0_0/0.9)]"
    >
      <FocusGroup focusKey="next-up" boundary autoFocus className="flex flex-col gap-3">
        <div className="flex gap-3">
          <div className="relative aspect-video w-28 shrink-0 overflow-hidden rounded-lg bg-white/10">{art && <img src={art} alt="" className="size-full object-cover" />}</div>
          <div className="flex min-w-0 flex-col justify-center gap-1">
            <span className="flex items-center gap-1 text-[0.6875rem] font-semibold tracking-wide text-white/55 uppercase">
              Up next
              {countdown !== null && (
                <>
                  {" "}in <SlidingNumber number={countdown} className="tabular-nums" />s
                </>
              )}
            </span>
            <span className="line-clamp-2 font-semibold">
              {episodeLabel(next)} · {next.title}
            </span>
          </div>
        </div>
        <div className="flex gap-2">
          <Button variant="primary" size="sm" icon={Play} iconFilled onClick={onPlay} className="relative overflow-hidden">
            Play Now
          </Button>
          <Button variant="ghost" size="sm" onClick={onDismiss}>
            Keep Watching
          </Button>
        </div>
      </FocusGroup>
    </motion.div>
  );
}

function PanelButton({ tab, icon, label, panel, setPanel }: { tab: PanelTab; icon: LucideIcon; label: string; panel: PanelTab | null; setPanel: (t: PanelTab | null) => void }) {
  const open = panel === tab;
  return <Button variant="ghost" size="icon-sm" icon={icon} label={label} onClick={() => setPanel(open ? null : tab)} className={open ? "bg-white/20 text-white" : undefined} />;
}

export function PlayerView({ itemId, startMs }: { itemId: string; startMs: number }) {
  const navigate = useNavigate();
  const settings = useSettings();
  const frame = useMode((s) => s.frame);
  const store = useMemo(() => createPlayerStore(), []);
  const phase = store.state((s) => s.phase);
  const started = store.state((s) => s.started);
  const duration = store.state((s) => s.duration);
  const error = store.state((s) => s.error);
  const ended = store.state((s) => s.ended);

  const item = useQuery({ queryKey: ["item", itemId], queryFn: () => api.item(itemId) });
  const markers = useQuery({ queryKey: ["markers", itemId], queryFn: () => api.markers(itemId).catch(() => [] as Marker[]) });
  const adjacent = useQuery({ queryKey: ["adjacent", itemId], queryFn: () => api.adjacent(itemId).catch(() => null) });

  const [chrome, setChrome] = useState(true);
  const [panel, setPanel] = useState<PanelTab | null>(null);
  const [scrub, setScrub] = useState<number | null>(null);
  const [nextDismissed, setNextDismissed] = useState(false);
  const [countdown, setCountdown] = useState<number | null>(null);
  const hideTimer = useRef<number | undefined>(undefined);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  const panelRef = useRef(panel);
  panelRef.current = panel;

  const poke = useCallback(() => {
    setChrome(true);
    window.clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(() => {
      if (phaseRef.current === "playing" && !panelRef.current) setChrome(false);
    }, HIDE_AFTER);
  }, []);

  const leave = useCallback(() => navigate(-1), [navigate]);

  // Session lifecycle.
  useEffect(() => {
    const fit = () => void api.playerViewport({ x: 0, y: 0, width: window.innerWidth, height: window.innerHeight });
    let unlisten: (() => void) | undefined;
    let alive = true;
    void onPlayerEvent(store.apply).then((u) => (alive ? (unlisten = u) : u()));
    fit();
    window.addEventListener("resize", fit);
    window.addEventListener("mousemove", poke);
    poke();
    api
      .play({ item: itemId, sourceId: null, startMs: startMs || null, audio: { type: "auto" }, subtitle: { type: "auto" } })
      .catch((e) => store.setError(asError(e).message));
    requestAnimationFrame(() => focusKey(TIMELINE_KEY));
    return () => {
      alive = false;
      unlisten?.();
      window.removeEventListener("resize", fit);
      window.removeEventListener("mousemove", poke);
      window.clearTimeout(hideTimer.current);
      store.dispose();
      cmd({ type: "stop" });
    };
  }, [store, itemId, startMs, poke]);

  // Paused or loading keeps the controls up.
  useEffect(() => {
    if (phase !== "playing") setChrome(true);
    else poke();
  }, [phase, poke]);

  const seekTo = useCallback(
    (ms: number) => {
      store.seekTo(ms);
      cmd({ type: "seekAbsolute", ms: Math.round(ms) });
    },
    [store],
  );
  const seekBy = useCallback(
    (ms: number) => {
      store.seekTo(store.position.get() + ms);
      cmd({ type: "seekRelative", ms });
    },
    [store],
  );

  // ---- markers: skip intro / recap ----------------------------------------
  const pos = useSecond(store.position);
  const activeMarker = markers.data?.find((m) => (m.kind === "intro" || m.kind === "recap") && pos >= m.startMs && pos < m.endMs - 1000);
  const autoSkip = settings?.playback.skipIntro === "auto";
  const showSkip = !!activeMarker && settings?.playback.skipIntro !== "off" && !autoSkip;
  useEffect(() => {
    if (activeMarker && autoSkip) seekTo(activeMarker.endMs);
  }, [activeMarker, autoSkip, seekTo]);

  // ---- next episode ----------------------------------------------------------
  const next = adjacent.data?.next ?? null;
  const creditsStart = markers.data?.find((m) => m.kind === "credits")?.startMs;
  const showNext = !!next && !nextDismissed && !!duration && settings?.notifications.nextEpisode !== false && pos >= (creditsStart ?? duration - NEXT_UP_WINDOW);
  const playNext = useCallback(() => {
    if (next) navigate(playPath(next.id, 0), { replace: true });
  }, [next, navigate]);

  useEffect(() => {
    if (showNext && settings?.playback.autoplayNext && countdown === null) setCountdown(settings.playback.autoplayCountdownSecs);
  }, [showNext, settings, countdown]);
  useEffect(() => {
    if (countdown === null) return;
    if (countdown <= 0) {
      setCountdown(null);
      playNext();
      return;
    }
    const t = window.setTimeout(() => setCountdown((c) => (c === null ? null : c - 1)), 1000);
    return () => window.clearTimeout(t);
  }, [countdown, playNext]);
  useEffect(() => {
    if (!ended) return;
    if (next && settings?.playback.autoplayNext) playNext();
    else leave();
  }, [ended, next, settings, playNext, leave]);

  // ---- remote/keyboard -------------------------------------------------------
  useEffect(() => {
    return onAction((a) => {
      const wasHidden = !chrome;
      poke();
      if (panel) {
        if (a.type === "back") {
          setPanel(null);
          requestAnimationFrame(() => focusKey(TIMELINE_KEY));
          return true;
        }
        return a.type === "playPause" ? (cmd({ type: "togglePause" }), true) : false;
      }
      switch (a.type) {
        case "playPause":
          cmd({ type: "togglePause" });
          return true;
        case "seek":
          seekBy(a.seconds * 1000);
          return true;
        case "move": {
          const onTimeline = document.activeElement?.getAttribute("role") === "slider" && document.activeElement?.getAttribute("aria-label") === "Position";
          if ((wasHidden || onTimeline) && (a.dir === "left" || a.dir === "right")) {
            seekBy(a.dir === "left" ? -SEEK_STEP : SEEK_STEP);
            return true;
          }
          // The first press only reveals the controls.
          if (wasHidden) {
            focusKey(TIMELINE_KEY);
            return true;
          }
          if (onTimeline && a.dir === "up") {
            setPanel("info");
            return true;
          }
          return false;
        }
        case "activate":
          if (wasHidden) {
            cmd({ type: "togglePause" });
            return true;
          }
          return false;
        case "back":
          leave();
          return true;
        default:
          return false;
      }
    });
  }, [chrome, panel, poke, seekBy, leave]);

  const it = item.data;
  const title = it?.episode ? (it.episode.seriesTitle ?? it.title) : (it?.title ?? "");
  const subtitle = it?.episode ? `${episodeLabel(it)} · ${it.title}` : it?.year ? String(it.year) : "";
  const visible = chrome || phase !== "playing" || !!panel;
  const scrubbing = scrub !== null;

  return (
    <div
      className={cn("fixed inset-0 overflow-hidden text-white select-none", !visible && "cursor-none")}
      style={{ "--bar-h": "8.5rem" } as CSSProperties}
      onClick={poke}
    >
      {/* Before the first frame, the artwork stands in for the video. */}
      <AnimatePresence>
        {!started && it && (
          <motion.div key="poster" className="absolute inset-0 bg-black" exit={{ opacity: 0 }} transition={{ duration: 0.6 }}>
            {imageUrl(it.images.backdrop ?? it.images.thumb, "hero") && (
              <img src={imageUrl(it.images.backdrop ?? it.images.thumb, "hero")} alt="" className="size-full object-cover opacity-40 blur-sm" />
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {(phase === "loading" || phase === "buffering") && !error && (
        <div className="absolute inset-0 grid place-items-center" aria-live="polite">
          <div className="grid size-14 place-items-center rounded-full bg-black/45">
            <Spinner />
          </div>
        </div>
      )}

      {/* Scrims keep the controls readable over bright video. */}
      <motion.div
        aria-hidden
        className="pointer-events-none absolute inset-0 bg-[linear-gradient(to_bottom,rgb(0_0_0/0.45),transparent_16%,transparent_62%,rgb(0_0_0/0.72))]"
        animate={{ opacity: visible ? 1 : 0 }}
        transition={{ duration: 0.4 }}
      />

      <AnimatePresence>
        {visible && (
          <motion.header
            key="top"
            initial={{ opacity: 0, y: -12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -12 }}
            transition={{ duration: 0.3 }}
            className="absolute top-0 left-0 z-50 flex items-center gap-4 px-8 pt-6"
          >
            <FocusGroup className="flex">
              <Button variant="ghost" size="icon-sm" icon={ChevronLeft} label="Back" onClick={leave} className="bg-black/30" />
            </FocusGroup>
          </motion.header>
        )}
      </AnimatePresence>

      <AnimatePresence>
        {showSkip && activeMarker && (
          <motion.div
            key="skip"
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 12 }}
            className="absolute right-8 bottom-[calc(var(--bar-h)+1rem)]"
          >
            <FocusGroup focusKey="skip" autoFocus>
              <Button variant="primary" size="md" onClick={() => seekTo(activeMarker.endMs)}>
                {activeMarker.kind === "recap" ? "Skip Recap" : "Skip Intro"}
              </Button>
            </FocusGroup>
          </motion.div>
        )}
        {showNext && next && !showSkip && (
          <NextUp
            key="next"
            next={next}
            countdown={countdown}
            onPlay={playNext}
            onDismiss={() => {
              setCountdown(null);
              setNextDismissed(true);
              focusKey(TIMELINE_KEY);
            }}
          />
        )}
      </AnimatePresence>

      <TitleBar hidden={!visible} />

      {/* Bottom bar: a quiet title, the scrubber between its times, then the
          transport centred with volume on the left and options on the right. */}
      <motion.footer
        className="absolute inset-x-0 bottom-0 flex flex-col gap-2 px-8 pb-5"
        animate={{ opacity: visible ? 1 : 0, y: visible ? 0 : 16 }}
        transition={{ type: "spring", stiffness: 300, damping: 34 }}
        style={{ pointerEvents: visible ? "auto" : "none" }}
      >
        <AnimatePresence>{panel && <PlayerPanel key="panel" tab={panel} onTab={setPanel} store={store} />}</AnimatePresence>

        <div className="flex min-w-0 items-baseline gap-2 drop-shadow-[0_1px_8px_rgb(0_0_0/0.6)]">
          <span className="truncate text-[0.9375rem] font-semibold">{title}</span>
          {subtitle && <span className="truncate text-[0.8125rem] text-white/60">{subtitle}</span>}
        </div>

        <FocusGroup focusKey="player-timeline-row" className="flex items-center gap-3 text-xs font-medium text-white/70">
          {scrubbing ? <span className="w-14 tabular-nums">{clock(scrub)}</span> : <Clock value={store.position} className="w-14" />}
          <Timeline store={store} markers={markers.data ?? []} onSeek={seekTo} onScrub={setScrub} />
          <span className="w-14 text-right">{duration > 0 && <Clock value={store.position} remainingOf={duration} />}</span>
        </FocusGroup>

        <FocusGroup focusKey="player-controls" className="grid grid-cols-[1fr_auto_1fr] items-center gap-4">
          <div className="flex items-center">
            <VolumeControl store={store} />
          </div>
          <div className="flex items-center gap-1.5">
            {adjacent.data?.previous && (
              <Button variant="ghost" size="icon-sm" icon={SkipBack} label="Previous episode" onClick={() => navigate(playPath(adjacent.data!.previous!.id), { replace: true })} />
            )}
            <Button variant="ghost" size="icon-sm" icon={RotateCcw} label="Back 10 seconds" onClick={() => seekBy(-SEEK_STEP)} />
            <Button variant="ghost" size="icon" icon={phase === "paused" ? Play : Pause} iconFilled label={phase === "paused" ? "Play" : "Pause"} onClick={() => cmd({ type: "togglePause" })} />
            <Button variant="ghost" size="icon-sm" icon={RotateCw} label="Forward 10 seconds" onClick={() => seekBy(SEEK_STEP)} />
            {next && <Button variant="ghost" size="icon-sm" icon={SkipForward} label="Next episode" onClick={playNext} />}
          </div>
          <div className="flex items-center justify-end gap-1">
            <PanelButton tab="info" icon={Info} label="Playback information" panel={panel} setPanel={setPanel} />
            <PanelButton tab="audio" icon={Languages} label="Audio" panel={panel} setPanel={setPanel} />
            <PanelButton tab="sub" icon={Subtitles} label="Subtitles" panel={panel} setPanel={setPanel} />
            <Button variant="ghost" size="icon-sm" icon={frame ? Minimize : Maximize} label={frame ? "Exit full screen" : "Full screen"} onClick={() => void toggleFrame()} />
          </div>
        </FocusGroup>
      </motion.footer>

      {error && (
        <div className="absolute inset-0 grid place-items-center bg-black/70">
          <FocusGroup focusKey="player-error" boundary autoFocus className="flex max-w-lg flex-col items-center gap-4 text-center">
            <h2 className="text-2xl font-bold tracking-tight">Playback stopped</h2>
            <p className="text-white/75">{error}</p>
            <Button variant="primary" onClick={leave}>
              Back
            </Button>
          </FocusGroup>
        </div>
      )}
    </div>
  );
}
