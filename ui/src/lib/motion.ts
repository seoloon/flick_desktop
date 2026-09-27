// Shared motion vocabulary. Springs everywhere a user action causes movement;
// tuned to settle fast without overshoot you can read as "bouncy".
import type { Transition } from "motion/react";

/** Focus lift, presses, small state changes. */
export const focusSpring: Transition = { type: "spring", stiffness: 420, damping: 32, mass: 0.7 };
/** Panels, sheets and layout moves. */
export const panelSpring: Transition = { type: "spring", stiffness: 260, damping: 30 };
/** Highlights sliding between tabs or segments. */
export const pillSpring: Transition = { type: "spring", stiffness: 500, damping: 40 };
/** Content entering a screen. */
export const enter: Transition = { duration: 0.45, ease: [0.32, 0.72, 0, 1] };
/** The ambient backdrop crossfade: slow, never the focus of attention. */
export const ambientFade: Transition = { duration: 1.1, ease: [0.4, 0, 0.2, 1] };
