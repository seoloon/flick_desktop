// Launch animation over the app, which starts underneath and loads while it
// plays (at start-up, and when switching to Flick Frame). It always plays
// to the end, then fades out; input during it never reaches the app below.
import { Player, type PlayerRef } from "@remotion/player";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { useIntro } from "@/lib/intro";
import { onAction, onKey } from "@/nav/input";
import { FLICK_INTRO, FlickIntro, type FlickIntroProps } from "./FlickIntro";

type LaunchIntroProps = {
  /** Called once the intro has played and faded (or right away when it is skipped). */
  onDone: () => void;
  variant?: FlickIntroProps["variant"];
  /** Skip it, e.g. when Performance > Animations is 0. Reduced motion always skips. */
  skip?: boolean;
};

/** The hand-off to the app: the last frame is the page colour, this reveals what is behind. */
const FADE = { duration: 0.3, ease: [0.4, 0, 0.2, 1] } as const;

/** Full-window launch animation; unmount it in `onDone`. */
export function LaunchIntro({ onDone, variant = "word", skip = false }: LaunchIntroProps) {
  const ref = useRef<PlayerRef>(null);
  const [skipped] = useState(() => skip || window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  // Held while the window goes fullscreen: the player is already there,
  // hidden and paused, measuring itself as the window grows, so that when
  // it is released it only has to play (a player appearing then would draw
  // its first frames before it has measured its new size).
  const held = useIntro((s) => s.held);
  // The composition takes the window's size: everything scales with its height,
  // so there is no letterboxing whatever the aspect ratio.
  const [size, setSize] = useState(() => ({ width: window.innerWidth, height: window.innerHeight }));
  useEffect(() => {
    const resize = () => setSize({ width: window.innerWidth, height: window.innerHeight });
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, []);
  const [leaving, setLeaving] = useState(false);
  const done = useRef(onDone);
  done.current = onDone;

  useEffect(() => {
    if (skipped) done.current();
  }, [skipped]);

  useEffect(() => {
    if (skipped) return;
    // Not skippable: keys (before they become actions) and controller
    // actions are swallowed so nothing moves underneath.
    const offKey = onKey(() => true);
    const offAction = onAction(() => true);
    return () => {
      offKey();
      offAction();
    };
  }, [skipped]);

  useEffect(() => {
    if (skipped) return;
    const leave = () => setLeaving(true);
    const player = ref.current;
    player?.addEventListener("ended", leave);
    return () => player?.removeEventListener("ended", leave);
  }, [skipped]);

  useEffect(() => {
    if (!skipped && !held) ref.current?.play();
  }, [skipped, held]);

  if (skipped) return null;
  return (
    <AnimatePresence onExitComplete={() => done.current()}>
      {!leaving && (
        <motion.div key="intro" exit={{ opacity: 0 }} transition={FADE} className="fixed inset-0 z-[9999] cursor-default bg-background">
          <Player
            ref={ref}
            component={FlickIntro}
            inputProps={{ variant }}
            durationInFrames={FLICK_INTRO.durationInFrames}
            fps={FLICK_INTRO.fps}
            compositionWidth={size.width}
            compositionHeight={size.height}
            controls={false}
            clickToPlay={false}
            doubleClickToFullscreen={false}
            spaceKeyToPlayOrPause={false}
            style={{ width: "100%", height: "100%", visibility: held ? "hidden" : "visible" }}
          />
        </motion.div>
      )}
    </AnimatePresence>
  );
}
