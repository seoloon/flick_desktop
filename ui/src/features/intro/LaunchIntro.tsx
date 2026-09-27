// Launch animation over the app, which starts underneath and loads while it
// plays. It fades out when it ends; a click, a key or a controller button
// skips it, and that input never reaches the app below.
import { Player, type PlayerRef } from "@remotion/player";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
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
  // The composition takes the window's size: everything scales with its height,
  // so there is no letterboxing whatever the aspect ratio.
  const [size] = useState(() => ({ width: window.innerWidth, height: window.innerHeight }));
  const [leaving, setLeaving] = useState(false);
  const done = useRef(onDone);
  done.current = onDone;

  useEffect(() => {
    if (skipped) done.current();
  }, [skipped]);

  useEffect(() => {
    if (skipped) return;
    const leave = () => setLeaving(true);
    const player = ref.current;
    player?.addEventListener("ended", leave);
    // Keys first (before they become actions), then controller actions.
    const offKey = onKey(() => {
      leave();
      return true;
    });
    const offAction = onAction(() => {
      leave();
      return true;
    });
    return () => {
      player?.removeEventListener("ended", leave);
      offKey();
      offAction();
    };
  }, [skipped]);

  if (skipped) return null;
  return (
    <AnimatePresence onExitComplete={() => done.current()}>
      {!leaving && (
        <motion.div key="intro" exit={{ opacity: 0 }} transition={FADE} onPointerDown={() => setLeaving(true)} className="fixed inset-0 z-[9999] cursor-default">
          <Player
            ref={ref}
            component={FlickIntro}
            inputProps={{ variant }}
            durationInFrames={FLICK_INTRO.durationInFrames}
            fps={FLICK_INTRO.fps}
            compositionWidth={size.width}
            compositionHeight={size.height}
            autoPlay
            controls={false}
            clickToPlay={false}
            doubleClickToFullscreen={false}
            spaceKeyToPlayOrPause={false}
            style={{ width: "100%", height: "100%" }}
          />
        </motion.div>
      )}
    </AnimatePresence>
  );
}
