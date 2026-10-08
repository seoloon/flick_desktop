import React from "react";
import {
  AbsoluteFill,
  Easing,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { BEHIND_STEM, MARK_H, MARK_W, METAL, MID, SLANT, STEM, TOP } from "./mark";
import { WORD_EM, WORD_LETTERS, WORD_MARK_SCALE, WORD_METAL, WORD_VIEWBOX } from "./wordmark";

// Launch animation: the F builds itself (the stem rises, both bars flick out
// from behind it), slides aside for the letters, catches one specular sweep,
// then clears for the app. Monochrome: colour belongs to the artwork.
//
// Self-contained on purpose (only `remotion` hooks, no assets), so the folder
// can be dropped into the app and played with `@remotion/player`.
//
// Drawn from the shapes of `export/flick-wordmark.svg` (wordmark.ts): the F is
// the mark, "lick" completes it. No font, no measuring.

export const FLICK_INTRO = {
  fps: 60,
  durationInFrames: 153, // 2.55 s
  width: 1920,
  height: 1080,
} as const;

export type FlickIntroProps = {
  /** Page colour; the app's `--background` by default. "transparent" works too. */
  background?: string;
};

const EASE = Easing.bezier(0.76, 0, 0.24, 1);
const EASE_OUT = Easing.bezier(0.16, 1, 0.3, 1);
const EASE_IN = Easing.bezier(0.7, 0, 0.84, 0);

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
const ramp = (t: number, t0: number, t1: number, easing = EASE) => easing(clamp01((t - t0) / (t1 - t0)));

// Timeline, in seconds.
const T = {
  stem: [0.1, 0.8],
  top: 0.4,
  mid: 0.52,
  slide: [1.0, 1.6],
  letters: 1.06, // + 45 ms per letter, 0.6 s each
  sweep: [1.45, 2.0],
  out: [2.15, 2.55],
} as const;

const BAR_SPRING = { damping: 17, stiffness: 190, mass: 0.9 };
const MARK_PX = 150; // the mark is drawn 150 px tall at 1080p



// --- Shared: timeline values, the F, the stage ----------------------------
const useFlick = () => {
  const frame = useCurrentFrame();
  const { fps, height } = useVideoConfig();
  const t = frame / fps;
  const u = height / 1080;

  // Stem: rises out of the baseline along its own slant.
  const stemAt = (s: number) => 1 - ramp(s, T.stem[0], T.stem[1], EASE_OUT);
  const stemRest = stemAt(t);
  const stemV = (stemAt(t - 1 / fps) - stemRest) * MARK_H; // mark units / frame

  // Bars: flick out from behind the stem on a spring (a hair of overshoot).
  const bar = (start: number, travel: number) => {
    const at = (f: number) => spring({ frame: f - start * fps, fps, config: BAR_SPRING });
    const p = at(frame);
    return { x: -(1 - p) * travel, v: (p - at(frame - 1)) * travel };
  };

  const slide = ramp(t, T.slide[0], T.slide[1]);
  const settle = 0.965 + 0.035 * ramp(t, 0, 1.7, EASE_OUT);
  const out = ramp(t, T.out[0], T.out[1], EASE_IN);
  return {
    t,
    u,
    stemRest,
    stemV,
    stemD: MARK_H * 1.04,
    top: bar(T.top, 290),
    mid: bar(T.mid, 250),
    slide,
    out,
    wordScale: settle * (1 + 0.035 * out),
    glow: ramp(t, 0.1, 1.3, EASE_OUT) * (1 - out),
  };
};
type Flick = ReturnType<typeof useFlick>;

const blurX = (v: number) => Math.min(Math.abs(v) * 0.32, 22);

const Stage: React.FC<{ background: string; u: number; glow: number; children: React.ReactNode }> = ({
  background,
  u,
  glow,
  children,
}) => (
  <AbsoluteFill style={{ background, overflow: "hidden" }}>
    <AbsoluteFill
      style={{
        opacity: glow,
        background: `radial-gradient(ellipse ${760 * u}px ${440 * u}px at 50% 50%, rgba(255,255,255,0.075), rgba(255,255,255,0.02) 45%, transparent 75%)`,
      }}
    />
    {children}
  </AbsoluteFill>
);

/** Gradients, clips and blurs of the F. `sweepX1/2` are in the F's own (mark) units. */
const FDefs: React.FC<{ fl: Flick; sweepX1: number; sweepX2: number }> = ({ fl, sweepX1, sweepX2 }) => (
  <>
    {(Object.keys(METAL) as (keyof typeof METAL)[]).map((k) => (
      <linearGradient key={k} id={`flick-${k}`} x1="0" y1="0" x2="0" y2="1">
        {METAL[k].map(([o, c]) => (
          <stop key={o} offset={o} stopColor={c} />
        ))}
      </linearGradient>
    ))}
    <SweepGradient id="flick-sweep" x1={sweepX1} x2={sweepX2} />
    <clipPath id="flick-floor">
      <rect x={-400} y={-400} width={1400} height={MARK_H + 400} />
    </clipPath>
    <clipPath id="flick-behind-stem">
      <polygon points={BEHIND_STEM} />
    </clipPath>
    <MotionBlur id="flick-blur-stem" x={blurX(fl.stemV) * SLANT} y={blurX(fl.stemV)} />
    <MotionBlur id="flick-blur-top" x={blurX(fl.top.v)} y={0} />
    <MotionBlur id="flick-blur-mid" x={blurX(fl.mid.v)} y={0} />
  </>
);

/** The F itself, in mark units: three metal pieces, then the sweep over them. */
const FShapes: React.FC<{ fl: Flick }> = ({ fl }) => {
  const stemT = `translate(${-SLANT * fl.stemRest * fl.stemD} ${fl.stemRest * fl.stemD})`;
  return (
    <>
      <g clipPath="url(#flick-floor)">
        <polygon points={STEM} fill="url(#flick-stem)" transform={stemT} filter={fl.stemV > 0.05 ? "url(#flick-blur-stem)" : undefined} />
      </g>
      <g clipPath="url(#flick-behind-stem)">
        <polygon
          points={TOP}
          fill="url(#flick-top)"
          transform={`translate(${fl.top.x} 0)`}
          filter={Math.abs(fl.top.v) > 0.05 ? "url(#flick-blur-top)" : undefined}
        />
        <polygon
          points={MID}
          fill="url(#flick-mid)"
          transform={`translate(${fl.mid.x} 0)`}
          filter={Math.abs(fl.mid.v) > 0.05 ? "url(#flick-blur-mid)" : undefined}
        />
      </g>
      {/* Unclipped on purpose: bars still hidden behind the stem are shaded where they will land. */}
      <g fill="url(#flick-sweep)">
        <polygon points={STEM} transform={stemT} clipPath="url(#flick-floor)" />
        <g clipPath="url(#flick-behind-stem)">
          <polygon points={TOP} transform={`translate(${fl.top.x} 0)`} />
          <polygon points={MID} transform={`translate(${fl.mid.x} 0)`} />
        </g>
      </g>
    </>
  );
};

// --- The intro --------------------------------------------------------------
export const FlickIntro: React.FC<FlickIntroProps> = ({ background = "oklch(0.11 0 0)" }) => {
  const fl = useFlick();
  const { t, u } = fl;
  const { x: vx, y: vy, w: vw, h: vh } = WORD_VIEWBOX;
  const k = WORD_MARK_SCALE;
  const s = (MARK_PX * u) / MARK_H; // px per viewBox unit
  const markRight = MARK_W * k;

  // The mark alone sits in the middle, then slides aside for the letters.
  const shift = ((1 - fl.slide) * (vw - markRight) * s) / 2;

  // One band crosses the whole wordmark (px from its left edge). It lives in
  // viewBox units, so the F and the letters share a single continuous light.
  const bandW = 90 * u;
  const sweepP = ramp(t, T.sweep[0], T.sweep[1], Easing.bezier(0.45, 0, 0.3, 1));
  const band = -2.2 * bandW + sweepP * (vw * s + 4.4 * bandW);
  const gx1 = vx + (band - 2 * bandW) / s;
  const gx2 = vx + (band + 2 * bandW) / s;
  // The F sits in a scaled group: bring the band into its local units.
  const toF = (g: number) => (g - SLANT * MARK_H * (1 - k)) / k;

  // Letters arrive one after the other: fade, slide in from the left, unblur.
  const letter = (i: number) => ramp(t, T.letters + i * 0.045, T.letters + i * 0.045 + 0.6, EASE_OUT);

  return (
    <Stage background={background} u={u} glow={fl.glow}>
      <svg
        width={vw * s}
        height={vh * s}
        viewBox={`${vx} ${vy} ${vw} ${vh}`}
        style={{
          position: "absolute",
          left: "50%",
          top: "50%",
          overflow: "visible",
          transform: `translate(-50%, -50%) translateX(${shift}px) scale(${fl.wordScale})`,
          opacity: 1 - fl.out,
          filter: fl.out > 0 ? `blur(${fl.out * 6 * u}px)` : undefined,
        }}
      >
        <defs>
          <FDefs fl={fl} sweepX1={toF(gx1)} sweepX2={toF(gx2)} />
          <linearGradient id="flick-word" gradientUnits="userSpaceOnUse" x1="0" y1={WORD_METAL.y1} x2="0" y2={WORD_METAL.y2}>
            {WORD_METAL.stops.map(([o, c]) => (
              <stop key={o} offset={o} stopColor={c} />
            ))}
          </linearGradient>
          <SweepGradient id="flick-sweep-word" x1={gx1} x2={gx2} />
          {WORD_LETTERS.map((_, i) => {
            const b = ((1 - letter(i)) * 14 * u) / s;
            return <MotionBlur key={i} id={`flick-blur-l${i}`} x={b} y={b} />;
          })}
        </defs>

        {/* The F: the mark, cut to the height of the letters, on the baseline. */}
        <g transform={`translate(0 ${MARK_H * (1 - k)}) scale(${k})`}>
          <FShapes fl={fl} />
        </g>

        {WORD_LETTERS.map((d, i) => {
          const p = letter(i);
          return (
            <g
              key={i}
              opacity={p}
              transform={`translate(${-(1 - p) * 0.32 * WORD_EM} 0)`}
              filter={p < 1 ? `url(#flick-blur-l${i})` : undefined}
            >
              <path d={d} fill="url(#flick-word)" />
              <path d={d} fill="url(#flick-sweep-word)" />
            </g>
          );
        })}
      </svg>
    </Stage>
  );
};

// The light, across 4 band widths: nothing behind it, a white peak, a darker
// metal ahead of it (the pad beyond the last stop keeps that shade).
const LIGHT = [
  [0, "#ffffff", 0],
  [0.3, "#ffffff", 0],
  [0.5, "#ffffff", 0.9],
  [0.72, "#000000", 0.3],
  [1, "#000000", 0.3],
] as const;

/** The band as an SVG gradient, leaning like the F. x1/x2 are at y = 0 of the element's own units. */
const SweepGradient: React.FC<{ id: string; x1: number; x2: number }> = ({ id, x1, x2 }) => (
  <linearGradient
    id={id}
    gradientUnits="userSpaceOnUse"
    x1={x1}
    x2={x2}
    y1={0}
    y2={0}
    gradientTransform={`skewX(${-(Math.atan(SLANT) * 180) / Math.PI})`}
  >
    {LIGHT.map(([o, c, a]) => (
      <stop key={o} offset={o} stopColor={c} stopOpacity={a} />
    ))}
  </linearGradient>
);

const MotionBlur: React.FC<{ id: string; x: number; y: number }> = ({ id, x, y }) => (
  <filter id={id} x="-60%" y="-60%" width="220%" height="220%" colorInterpolationFilters="sRGB">
    <feGaussianBlur stdDeviation={`${x} ${y}`} />
  </filter>
);
