// Player state mirrored from Rust events. Position lives in a motion value,
// interpolated between the 4 Hz updates so the timeline moves at display rate
// without re-rendering React every frame.
import { motionValue } from "motion/react";
import { create } from "zustand";
import type { Chapter } from "@/ipc/bindings/Chapter";
import type { Phase } from "@/ipc/bindings/Phase";
import type { PlaybackDecision } from "@/ipc/bindings/PlaybackDecision";
import type { PlayerEvent } from "@/ipc/bindings/PlayerEvent";
import type { Track } from "@/ipc/bindings/Track";

const MAX_AHEAD_MS = 1000;
const STALE_MS = 500;

export type PlayerState = {
  phase: Phase;
  /** Became true once the first frame played: artwork placeholder goes away. */
  started: boolean;
  duration: number;
  buffered: number;
  volume: number;
  muted: boolean;
  tracks: Track[];
  chapters: Chapter[];
  decision: PlaybackDecision | null;
  error: string | null;
  ended: boolean;
};

export function createPlayerStore() {
  const state = create<PlayerState>(() => ({
    phase: "loading",
    started: false,
    duration: 0,
    buffered: 0,
    volume: 100,
    muted: false,
    tracks: [],
    chapters: [],
    decision: null,
    error: null,
    ended: false,
  }));
  const position = motionValue(0);

  let basePos = 0;
  let baseAt = performance.now();
  let raf = 0;
  const tick = () => {
    const { phase, duration } = state.getState();
    if (phase === "playing") {
      // mpv reports every 250 ms: never run more than a second ahead of it,
      // so a stalled engine shows a stopped timeline, not a moving one.
      const p = basePos + Math.min(performance.now() - baseAt, MAX_AHEAD_MS);
      position.set(duration ? Math.min(p, duration) : p);
    }
    raf = requestAnimationFrame(tick);
  };
  raf = requestAnimationFrame(tick);

  function apply(e: PlayerEvent) {
    switch (e.type) {
      case "state": {
        // State events also come from volume/mute changes, carrying a
        // position up to 250 ms old: while playing, never step back for that.
        const shown = position.get();
        const staleBackstep = e.phase === "playing" && state.getState().phase === "playing" && e.positionMs < shown && shown - e.positionMs < STALE_MS;
        basePos = staleBackstep ? shown : e.positionMs;
        baseAt = performance.now();
        position.set(basePos);
        state.setState((s) => ({
          phase: e.phase,
          started: s.started || e.phase === "playing",
          duration: e.durationMs ?? s.duration,
          buffered: e.bufferedMs ?? s.buffered,
          volume: e.volume,
          muted: e.muted,
        }));
        break;
      }
      case "tracks":
        state.setState({ tracks: e.tracks });
        break;
      case "chapters":
        state.setState({ chapters: e.chapters });
        break;
      case "decision":
        state.setState({ decision: e.decision });
        break;
      case "ended":
        state.setState({ ended: true });
        break;
      case "error":
        state.setState({ error: e.message, phase: "error" });
        break;
    }
  }

  /** Optimistic seek so the timeline jumps immediately. */
  function seekTo(ms: number) {
    const { duration } = state.getState();
    basePos = Math.max(0, duration ? Math.min(ms, duration) : ms);
    baseAt = performance.now();
    position.set(basePos);
  }

  return {
    state,
    position,
    apply,
    seekTo,
    setError: (message: string) => state.setState({ error: message, phase: "error" }),
    dispose: () => cancelAnimationFrame(raf),
  };
}

export type PlayerStore = ReturnType<typeof createPlayerStore>;
