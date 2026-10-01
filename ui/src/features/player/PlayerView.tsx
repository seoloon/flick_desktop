// Full-window player. The page is transparent: the native video layer is
// composited behind it by the OS (see ARCHITECTURE.md §4). This component only
// draws chrome and forwards intents to Rust.
//
// Remote model (tvOS): the scrubber has focus by default, left/right skip
// 10 s, Enter pauses, Up opens the settings menu, Down reaches the buttons. With
// the controls hidden, any key reveals them; left/right also skip at once.
//
// Picture in picture shrinks the whole window into a small always-on-top
// video (Rust `window_pip`); the video layer follows the window. Its controls
// are a compact overlay shown on hover, the rest of the window drags it.
import { useQuery } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ChevronLeft,
  ListVideo,
  Maximize,
  Maximize2,
  Minimize,
  Pause,
  PictureInPicture2,
  Play,
  RotateCcw,
  RotateCw,
  Settings,
  SkipBack,
  SkipForward,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import { AnimatePresence, motion, type MotionValue, useMotionValue, useMotionValueEvent, useSpring, useTransform } from "motion/react";
import { type CSSProperties, type MouseEvent, type PointerEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router";
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
import { setFrame, useMode } from "@/lib/mode";
import { useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { onAction } from "@/nav/input";
import { focusKey } from "@/nav/spatial";
import { ServerBadge } from "@/components/tv/ServerBadge";
import { useSources } from "@/lib/servers";
import { TitleBar } from "@/shell/TitleBar";
import { useWatch } from "../watch/store";
import { EpisodesPanel } from "./EpisodesPanel";
import { type MenuActions, PlayerMenu } from "./PlayerMenu";
import { playPath } from "./route";
import { createPlayerStore, type PlayerStore } from "./store";

const HIDE_AFTER = 3500;
const NEXT_UP_WINDOW = 30_000;
const SEEK_STEP = 10_000;
const TIMELINE_KEY = "player-timeline";
const EPISODES_BUTTON = "player-episodes-button";

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

function Timeline({
  store,
  markers,
  scrub,
  onSeek,
  onScrub,
  focusKey: key = TIMELINE_KEY,
}: {
  store: PlayerStore;
  markers: Marker[];
  scrub: number | null;
  onSeek: (ms: number) => void;
  onScrub: (ms: number | null) => void;
  focusKey?: string;
}) {
  const duration = store.state((s) => s.duration);
  const buffered = store.state((s) => s.buffered);
  const chapters = store.state((s) => s.chapters);
  const tv = useTv<HTMLDivElement>({ focusKey: key, scroll: false });
  const pct = (ms: number) => (duration ? `${Math.min(100, (ms / duration) * 100)}%` : "0%");
  // The bar shows the playhead, or the pointer while scrubbing, through a
  // stiff spring: seeks glide instead of teleporting, playback looks the same.
  const target = useMotionValue(store.position.get());
  useMotionValueEvent(store.position, "change", (p) => scrub === null && target.set(p));
  useEffect(() => target.set(scrub ?? store.position.get()), [scrub, target, store.position]);
  const smooth = useSpring(target, { stiffness: 380, damping: 42 });
  const fill = useTransform(smooth, (p) => pct(p));
  // Pointer position over the bar, in ms: drives the chapter tooltip.
  const [hoverMs, setHoverMs] = useState<number | null>(null);
  const hoverChapter = hoverMs === null ? -1 : chapters.reduce((found, c, i) => (c.startMs <= hoverMs ? i : found), -1);
  const chapter = hoverChapter >= 0 ? chapters[hoverChapter] : null;
  const msAt = (e: PointerEvent<HTMLDivElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    return Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * duration;
  };

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
      onPointerMove={(e) => chapters.length > 0 && duration > 0 && setHoverMs(msAt(e))}
      onPointerLeave={() => setHoverMs(null)}
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
      {chapter && hoverMs !== null && (
        <div
          className="pointer-events-none absolute bottom-full mb-3 flex max-w-64 -translate-x-1/2 flex-col items-center gap-0.5 rounded-xl bg-black/75 px-3 py-2 text-center shadow-[inset_0_0_0_1px_rgb(255_255_255/0.08),0_12px_30px_-10px_rgb(0_0_0/0.8)]"
          style={{ left: `clamp(4rem, ${pct(hoverMs)}, calc(100% - 4rem))` }}
        >
          <span className="line-clamp-2 text-[0.8125rem] font-semibold text-white">{chapter.title ?? `Chapter ${hoverChapter + 1}`}</span>
          <span className="text-xs tabular-nums text-white/60">{clock(chapter.startMs)}</span>
        </div>
      )}
    </div>
  );
}

function VolumeControl({ store }: { store: PlayerStore }) {
  const volume = store.state((s) => s.volume);
  const muted = store.state((s) => s.muted);
  const tv = useTv<HTMLDivElement>({ scroll: false });
  // The bar follows the hand at once; mpv catches up behind it. The draft is
  // dropped once changes stop and the engine has reported the new value.
  const [draft, setDraft] = useState<number | null>(null);
  const shown = draft ?? (muted ? 0 : volume);
  const fill = useSpring(shown, { stiffness: 520, damping: 44 });
  useEffect(() => fill.set(shown), [fill, shown]);
  const width = useTransform(fill, (v) => `${v}%`);
  const pending = useRef<number | null>(null);
  const release = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(release.current), []);

  const change = (v: number) => {
    const next = Math.max(0, Math.min(100, Math.round(v)));
    setDraft(next);
    if (pending.current === null) {
      requestAnimationFrame(() => {
        const value = pending.current;
        pending.current = null;
        if (value === null) return;
        if (muted && value > 0) cmd({ type: "setMute", muted: false });
        cmd({ type: "setVolume", volume: value });
      });
    }
    pending.current = next;
    window.clearTimeout(release.current);
    release.current = window.setTimeout(() => setDraft(null), 700);
  };

  useEffect(() => {
    if (!tv.focused) return;
    return onAction((a) => {
      if (a.type !== "move" || (a.dir !== "left" && a.dir !== "right")) return false;
      change(shown + (a.dir === "left" ? -5 : 5));
      return true;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tv.focused, shown]);

  const drag = (e: PointerEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const at = (x: number) => {
      const r = el.getBoundingClientRect();
      return Math.min(1, Math.max(0, (x - r.left - 12) / (r.width - 24))) * 100;
    };
    change(at(e.clientX));
    const move = (ev: globalThis.PointerEvent) => change(at(ev.clientX));
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  };
  return (
    <div className="flex items-center gap-1">
      <Button variant="ghost" size="icon-sm" icon={shown === 0 ? VolumeX : Volume2} label={muted ? "Unmute" : "Mute"} onClick={() => cmd({ type: "setMute", muted: !muted })} />
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
        initial="rest"
        whileHover="hover"
        animate={tv.showFocus ? "focus" : "rest"}
        variants={{ rest: { scale: 1 }, hover: { scale: 1 }, focus: { scale: 1.08 } }}
        transition={focusSpring}
        className={cn("group/vol flex h-9 w-24 cursor-pointer items-center rounded-full px-3 transition-colors duration-200", tv.showFocus && "bg-white")}
      >
        <motion.span
          variants={{ rest: { height: 4 }, hover: { height: 6 }, focus: { height: 6 } }}
          transition={focusSpring}
          className={cn("block w-full overflow-hidden rounded-full", tv.showFocus ? "bg-black/15" : "bg-white/25")}
        >
          <motion.span className={cn("block h-full rounded-full", tv.showFocus ? "bg-black" : "bg-white")} style={{ width }} />
        </motion.span>
      </motion.div>
    </div>
  );
}

function NextUp({ next, countdown, total, onPlay, onDismiss }: { next: MediaItem; countdown: number | null; total: number; onPlay: () => void; onDismiss: () => void }) {
  const art = imageUrl(next.images.thumb ?? next.images.backdrop, "card");
  return (
    <motion.div
      initial={{ opacity: 0, x: 40 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 40 }}
      transition={{ type: "spring", stiffness: 260, damping: 30 }}
      className="absolute right-8 bottom-[calc(var(--bar-h)+1rem)] w-[21rem] overflow-hidden rounded-3xl bg-black/75 p-3 text-sm shadow-[inset_0_1px_0_rgb(255_255_255/0.12),inset_0_0_0_1px_rgb(255_255_255/0.08),0_30px_60px_-20px_rgb(0_0_0/0.9)]"
    >
      <FocusGroup focusKey="next-up" boundary autoFocus className="flex flex-col gap-3">
        <div className="flex gap-3">
          <div className="relative aspect-video w-28 shrink-0 overflow-hidden rounded-xl bg-white/10">{art && <img src={art} alt="" className="size-full object-cover" />}</div>
          <div className="flex min-w-0 flex-col justify-center gap-1">
            <span className="text-[0.6875rem] font-semibold tracking-wide text-white/55 uppercase">Up next</span>
            <span className="line-clamp-2 font-semibold">
              {episodeLabel(next)} · {next.title}
            </span>
          </div>
        </div>
        <div className="flex gap-2">
          <Button variant="primary" size="sm" icon={Play} iconFilled onClick={onPlay} className="relative overflow-hidden">
            {/* Fills over the countdown: when it is full, the next episode starts. */}
            {countdown !== null && <motion.span aria-hidden className="absolute inset-0 origin-left bg-black/20" initial={{ scaleX: 0 }} animate={{ scaleX: 1 }} transition={{ duration: total, ease: "linear" }} />}
            <span className="relative">Next Episode</span>
          </Button>
          <Button variant="ghost" size="sm" onClick={onDismiss}>
            View Credits
          </Button>
        </div>
      </FocusGroup>
    </motion.div>
  );
}

/**
 * Picture-in-picture chrome: title and window actions on top, transport in the
 * middle, the timeline at the bottom, all shown while the pointer is over the
 * video (or when paused). Everything else drags the window.
 */
function PipOverlay({
  store,
  title,
  markers,
  visible,
  onSeek,
  onSeekBy,
  onExit,
  onClose,
}: {
  store: PlayerStore;
  title: string;
  markers: Marker[];
  visible: boolean;
  onSeek: (ms: number) => void;
  onSeekBy: (ms: number) => void;
  onExit: () => void;
  onClose: () => void;
}) {
  const phase = store.state((st) => st.phase);
  const duration = store.state((st) => st.duration);
  const [scrub, setScrub] = useState<number | null>(null);
  const small = "bg-black/40 text-white hover:bg-black/60 [&_svg]:size-4";
  return (
    <motion.div
      className="absolute inset-0 flex flex-col justify-between bg-black/35 p-2"
      animate={{ opacity: visible ? 1 : 0 }}
      transition={{ duration: 0.2 }}
      style={{ pointerEvents: visible ? "auto" : "none" }}
    >
      <div className="flex items-center gap-1">
        <span className="min-w-0 flex-1 truncate px-1.5 text-xs font-semibold drop-shadow-[0_1px_6px_rgb(0_0_0/0.7)]">{title}</span>
        <Button variant="ghost" size="icon-sm" icon={Maximize2} label="Back to the full player" onClick={onExit} className={small} />
        <Button variant="ghost" size="icon-sm" icon={X} label="Stop" onClick={onClose} className={small} />
      </div>
      <div className="flex items-center justify-center gap-2">
        <Button variant="ghost" size="icon-sm" icon={RotateCcw} label="Back 10 seconds" onClick={() => onSeekBy(-SEEK_STEP)} className={small} />
        <Button variant="ghost" size="icon" icon={phase === "paused" ? Play : Pause} iconFilled label={phase === "paused" ? "Play" : "Pause"} onClick={() => cmd({ type: "togglePause" })} className="bg-black/40 hover:bg-black/60" />
        <Button variant="ghost" size="icon-sm" icon={RotateCw} label="Forward 10 seconds" onClick={() => onSeekBy(SEEK_STEP)} className={small} />
      </div>
      <div className="flex items-center gap-2 px-1 text-[0.6875rem] font-medium text-white/80 drop-shadow-[0_1px_6px_rgb(0_0_0/0.7)]">
        {scrub !== null ? <span className="tabular-nums">{clock(scrub)}</span> : <Clock value={store.position} />}
        <Timeline store={store} markers={markers} scrub={scrub} onSeek={onSeek} onScrub={setScrub} focusKey="pip-timeline" />
        {duration > 0 && <Clock value={store.position} remainingOf={duration} />}
      </div>
    </motion.div>
  );
}

export function PlayerView({ itemId, startMs }: { itemId: string; startMs: number }) {
  const navigate = useNavigate();
  const settings = useSettings();
  // In a watch room the host chooses what plays next: nothing starts by itself.
  const inRoom = useWatch((w) => !!w.room);
  const frame = useMode((s) => s.frame);
  const store = useMemo(() => createPlayerStore(), []);
  const phase = store.state((s) => s.phase);
  const started = store.state((s) => s.started);
  const duration = store.state((s) => s.duration);
  const error = store.state((s) => s.error);
  const ended = store.state((s) => s.ended);

  const item = useQuery({ queryKey: ["item", itemId], queryFn: () => api.item(itemId) });
  const [source] = useSources([itemId]);
  const markers = useQuery({ queryKey: ["markers", itemId], queryFn: () => api.markers(itemId).catch(() => [] as Marker[]) });
  const adjacent = useQuery({ queryKey: ["adjacent", itemId], queryFn: () => api.adjacent(itemId).catch(() => null) });

  const [chrome, setChrome] = useState(true);
  const [menu, setMenu] = useState(false);
  const [episodes, setEpisodes] = useState(false);
  const menuActions: MenuActions = useRef(null);
  // Window fullscreen for this playback only; Flick Frame stays what the user chose.
  const [fullscreen, setFullscreen] = useState(false);
  const fullscreenRef = useRef(false);
  const [pip, setPip] = useState(false);
  const pipRef = useRef(false);
  // Flick Frame is off while in Picture in Picture, and back on when leaving it.
  const frameBeforePip = useRef(false);
  const [hover, setHover] = useState(false);
  const [scrub, setScrub] = useState<number | null>(null);
  const [nextDismissed, setNextDismissed] = useState(false);
  const [countdown, setCountdown] = useState<number | null>(null);
  const hideTimer = useRef<number | undefined>(undefined);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  const menuRef = useRef(menu);
  menuRef.current = menu || episodes;

  const poke = useCallback(() => {
    setChrome(true);
    window.clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(() => {
      if (phaseRef.current === "playing" && !menuRef.current) setChrome(false);
    }, HIDE_AFTER);
  }, []);

  const leave = useCallback(() => navigate(-1), [navigate]);

  const toggleFullscreen = useCallback(() => {
    // Flick Frame is already fullscreen: leaving fullscreen leaves the Frame.
    if (useMode.getState().frame) return void setFrame(false);
    const next = !fullscreenRef.current;
    fullscreenRef.current = next;
    setFullscreen(next);
    void api.setFullscreen(next).catch(() => undefined);
  }, []);
  // "Fullscreen on play": once, when this playback opens (Flick Frame is already fullscreen).
  const autoFullscreen = useRef(false);
  useEffect(() => {
    if (autoFullscreen.current || !settings) return;
    autoFullscreen.current = true;
    if (settings.playback.fullscreenOnPlay && !useMode.getState().frame && !fullscreenRef.current) toggleFullscreen();
  }, [settings, toggleFullscreen]);
  const setPipMode = useCallback((on: boolean) => {
    if (pipRef.current === on) return;
    pipRef.current = on;
    setPip(on);
    setMenu(false);
    setHover(false);
    void (async () => {
      if (on) frameBeforePip.current = useMode.getState().frame;
      await api.windowPip(on).catch(() => undefined);
      if (on && frameBeforePip.current) await setFrame(false, false);
      if (!on && frameBeforePip.current) {
        frameBeforePip.current = false;
        await setFrame(true, false);
      }
    })();
    if (!on) requestAnimationFrame(() => focusKey(TIMELINE_KEY));
  }, []);
  // Leaving the player gives the window back as it was: out of PiP first
  // (which restores a fullscreen window), then out of the player's fullscreen.
  useEffect(
    () => () => {
      void (async () => {
        if (pipRef.current) await api.windowPip(false).catch(() => undefined);
        if (frameBeforePip.current) await setFrame(true, false);
        else if (fullscreenRef.current && !useMode.getState().frame) await api.setFullscreen(false).catch(() => undefined);
      })();
    },
    [],
  );
  // In PiP the window is its own drag handle; a double click returns to the full player.
  const pipMouseDown = (e: MouseEvent<HTMLDivElement>) => {
    if (e.button !== 0 || (e.target as Element).closest("button, [role=slider]")) return;
    if (e.detail === 2) setPipMode(false);
    else void getCurrentWindow().startDragging();
  };

  // Session lifecycle.
  useEffect(() => {
    // A window drag-resize fires `resize` many times per frame: send the
    // video rectangle once per frame, and only when it changed (each one
    // resizes mpv's swapchain on Windows, re-lays the layer on macOS).
    let fitFrame = 0;
    let lastFit = "";
    const sendFit = () => {
      fitFrame = 0;
      const rect = { x: 0, y: 0, width: window.innerWidth, height: window.innerHeight };
      const key = `${rect.width}x${rect.height}@${window.devicePixelRatio}`;
      if (key === lastFit) return;
      lastFit = key;
      void api.playerViewport(rect);
    };
    const fit = () => {
      if (!fitFrame) fitFrame = requestAnimationFrame(sendFit);
    };
    let unlisten: (() => void) | undefined;
    let alive = true;
    void onPlayerEvent(store.apply).then((u) => (alive ? (unlisten = u) : u()));
    sendFit();
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
      cancelAnimationFrame(fitFrame);
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

  // ---- markers: skip intro / recap / credits ----------------------------------
  // With a next episode, the credits are the Up next card's job; the skip
  // button is for the last episode and for movies.
  const next = adjacent.data?.next ?? null;
  const pos = useSecond(store.position);
  const activeMarker = markers.data?.find((m) => (m.kind === "intro" || m.kind === "recap" || (m.kind === "credits" && !next)) && pos >= m.startMs && pos < m.endMs - 1000);
  const skipMode = activeMarker ? (activeMarker.kind === "credits" ? settings?.playback.skipCredits : settings?.playback.skipIntro) : "off";
  const autoSkip = skipMode === "auto";
  const showSkip = !!activeMarker && skipMode === "button";
  useEffect(() => {
    if (activeMarker && autoSkip) seekTo(activeMarker.endMs);
  }, [activeMarker, autoSkip, seekTo]);

  // ---- next episode ----------------------------------------------------------
  const creditsStart = markers.data?.find((m) => m.kind === "credits")?.startMs;
  const showNext = !!next && !nextDismissed && !!duration && settings?.notifications.nextEpisode !== false && pos >= (creditsStart ?? duration - NEXT_UP_WINDOW);
  const playNext = useCallback(() => {
    if (next) navigate(playPath(next.id, 0), { replace: true });
  }, [next, navigate]);

  useEffect(() => {
    if (showNext && !inRoom && settings?.playback.autoplayNext && countdown === null) setCountdown(settings.playback.autoplayCountdownSecs);
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
    if (next && !inRoom && settings?.playback.autoplayNext) playNext();
    else leave();
  }, [ended, next, settings, playNext, leave]);

  // ---- remote/keyboard -------------------------------------------------------
  useEffect(() => {
    return onAction((a) => {
      if (pip) {
        if (a.type === "playPause" || a.type === "activate") cmd({ type: "togglePause" });
        else if (a.type === "seek") seekBy(a.seconds * 1000);
        else if (a.type === "move" && (a.dir === "left" || a.dir === "right")) seekBy(a.dir === "left" ? -SEEK_STEP : SEEK_STEP);
        else if (a.type === "back") setPipMode(false);
        else return false;
        return true;
      }
      const wasHidden = !chrome;
      poke();
      if (episodes) {
        if (a.type === "back") {
          setEpisodes(false);
          requestAnimationFrame(() => focusKey(EPISODES_BUTTON));
          return true;
        }
        return a.type === "playPause" ? (cmd({ type: "togglePause" }), true) : false;
      }
      if (menu) {
        if (menuActions.current?.(a)) return true;
        if (a.type === "back") {
          setMenu(false);
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
            setMenu(true);
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
  }, [chrome, menu, episodes, pip, poke, seekBy, leave, setPipMode]);

  const it = item.data;
  const title = it?.episode ? (it.episode.seriesTitle ?? it.title) : (it?.title ?? "");
  const subtitle = it?.episode ? `${episodeLabel(it)} · ${it.title}` : it?.year ? String(it.year) : "";
  const visible = !pip && (chrome || phase !== "playing" || menu || episodes);
  const scrubbing = scrub !== null;

  return (
    <div
      className={cn("fixed inset-0 overflow-hidden text-white select-none", !visible && !pip && "[&_*]:cursor-none cursor-none")}
      style={{ "--bar-h": "8.5rem" } as CSSProperties}
      onClick={pip ? undefined : poke}
      onDoubleClick={(e) => !pip && e.target === e.currentTarget && toggleFullscreen()}
      onMouseDown={pip ? pipMouseDown : undefined}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
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

      {pip && (
        <PipOverlay
          store={store}
          title={title}
          markers={markers.data ?? []}
          visible={hover || phase === "paused"}
          onSeek={seekTo}
          onSeekBy={seekBy}
          onExit={() => setPipMode(false)}
          onClose={leave}
        />
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
        {showSkip && activeMarker && !pip && (
          <motion.div
            key="skip"
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 12 }}
            className="absolute right-8 bottom-[calc(var(--bar-h)+1rem)]"
          >
            <FocusGroup focusKey="skip" autoFocus>
              <Button variant="primary" size="md" onClick={() => seekTo(activeMarker.endMs)}>
                {activeMarker.kind === "recap" ? "Skip Recap" : activeMarker.kind === "credits" ? "Skip Credits" : "Skip Intro"}
              </Button>
            </FocusGroup>
          </motion.div>
        )}
        {showNext && next && !showSkip && !pip && (
          <NextUp
            key="next"
            next={next}
            countdown={countdown}
            total={settings?.playback.autoplayCountdownSecs ?? 5}
            onPlay={playNext}
            onDismiss={() => {
              setCountdown(null);
              setNextDismissed(true);
              focusKey(TIMELINE_KEY);
            }}
          />
        )}
      </AnimatePresence>

      {!fullscreen && !pip && <TitleBar hidden={!visible} />}

      {/* Bottom bar: a quiet title, the scrubber between its times, then the
          transport centred with volume on the left and options on the right. */}
      {!pip && (
        <motion.footer
          className="absolute inset-x-0 bottom-0 flex flex-col gap-2 px-8 pb-5"
          animate={{ opacity: visible ? 1 : 0, y: visible ? 0 : 16 }}
          transition={{ type: "spring", stiffness: 300, damping: 34 }}
          style={{ pointerEvents: visible ? "auto" : "none" }}
        >
          <AnimatePresence>
            {menu && <PlayerMenu key="menu" store={store} actions={menuActions} source={source} />}
            {episodes && it?.episode?.series && (
              <EpisodesPanel
                key="episodes"
                series={it.episode.series}
                season={it.episode.season}
                currentId={itemId}
                onPick={(id) => (id === itemId ? setEpisodes(false) : navigate(playPath(id, 0), { replace: true }))}
              />
            )}
          </AnimatePresence>

          <div className="flex min-w-0 items-baseline gap-2 drop-shadow-[0_1px_8px_rgb(0_0_0/0.6)]">
            <span className="truncate text-[0.9375rem] font-semibold">{title}</span>
            {subtitle && <span className="truncate text-[0.8125rem] text-white/60">{subtitle}</span>}
            {source && <ServerBadge server={source} quiet className="ml-1 text-[0.8125rem]" />}
          </div>

          <FocusGroup focusKey="player-timeline-row" className="flex items-center gap-3 text-xs font-medium text-white/70">
            {scrubbing ? <span className="w-14 tabular-nums">{clock(scrub)}</span> : <Clock value={store.position} className="w-14" />}
            <Timeline store={store} markers={markers.data ?? []} scrub={scrub} onSeek={seekTo} onScrub={setScrub} />
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
              {it?.episode?.series && (
                <Button
                  variant="ghost"
                  size="icon-sm"
                  icon={ListVideo}
                  label="Episodes"
                  focusKey={EPISODES_BUTTON}
                  onClick={() => {
                    setMenu(false);
                    setEpisodes((e) => !e);
                  }}
                  className={cn(episodes && "bg-white/20 text-white")}
                />
              )}
              <Button
                variant="ghost"
                size="icon-sm"
                icon={Settings}
                label="Audio, subtitles and playback info"
                onClick={() => {
                  setEpisodes(false);
                  setMenu((m) => !m);
                }}
                className={cn("[&_svg]:transition-transform [&_svg]:duration-500 [&_svg]:ease-apple", menu && "bg-white/20 text-white [&_svg]:rotate-90")}
              />
              <Button variant="ghost" size="icon-sm" icon={PictureInPicture2} label="Picture in Picture" onClick={() => setPipMode(true)} />
              <Button
                variant="ghost"
                size="icon-sm"
                icon={fullscreen || frame ? Minimize : Maximize}
                label={frame ? "Exit Flick Frame" : fullscreen ? "Exit full screen" : "Full screen"}
                onClick={toggleFullscreen}
              />
            </div>
          </FocusGroup>
        </motion.footer>
      )}

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
