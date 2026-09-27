import React, { useLayoutEffect, useRef, useState } from "react";
import {
  AbsoluteFill,
  continueRender,
  delayRender,
  Easing,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { BEHIND_STEM, MARK_H, MARK_W, METAL, MID, SLANT, STEM, TOP } from "./mark";

// Launch animation: the F builds itself (the stem rises, both bars flick out
// from behind it), slides aside for the wordmark, catches one specular sweep,
// then clears for the app. Monochrome: colour belongs to the artwork.
//
// Self-contained on purpose (only `remotion` hooks, no assets): played in
// the app by `LaunchIntro` through `@remotion/player`.

export const FLICK_INTRO = {
  fps: 60,
  durationInFrames: 153, // 2.55 s
  width: 1920,
  height: 1080,
} as const;

export type FlickIntroProps = {
  /** Page colour; the app's `--background` by default. "transparent" works too. */
  background?: string;
  /** "lockup": mark beside the word Flick. "word": the mark is the F of F·lick. */
  variant?: "lockup" | "word";
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
// Wordmark per variant. `size` is the font size (px at 1080p), `gap` the
// space between the mark and the first letter, `skew` an oblique that makes
// the letters lean like the F.
const WORDMARK = {
  lockup: { text: "Flick", size: 172, weight: 600, gap: 40, skew: 0, tracking: "-0.03em" },
  word: { text: "lick", size: 194, weight: 900, gap: -6, skew: 10, tracking: "-0.02em" },
} as const;
const WORD_ANGLE = 100; // CSS degrees: a band leaning like the F (10°)

const FONT =
  'var(--font-heading, -apple-system, BlinkMacSystemFont, "SF Pro Display", "Inter Variable", Inter, system-ui, sans-serif)';

export const FlickIntro: React.FC<FlickIntroProps> = ({ background = "oklch(0.11 0 0)", variant = "lockup" }) => {
  const frame = useCurrentFrame();
  const { fps, height } = useVideoConfig();
  const t = frame / fps;
  const u = height / 1080;

  // The wordmark is laid out by the browser (SF Pro or Inter), so its width
  // and letter positions are measured, not assumed.
  const wordRef = useRef<HTMLSpanElement>(null);
  const [word, setWord] = useState({ width: 0, lefts: [] as number[] });
  const [handle] = useState(() => delayRender("Measuring the Flick wordmark"));
  useLayoutEffect(() => {
    const measure = () => {
      const el = wordRef.current;
      if (!el) return;
      // offsetLeft is relative to the lockup (the positioned ancestor).
      const lefts = Array.from(el.children, (c) => (c as HTMLElement).offsetLeft - el.offsetLeft);
      setWord((w) => (w.width === el.offsetWidth && w.lefts.join() === lefts.join() ? w : { width: el.offsetWidth, lefts }));
    };
    measure();
    document.fonts.ready.then(() => {
      measure();
      continueRender(handle);
    });
  }, [handle, height, variant]);

  const markH = 150 * u;
  const markW = (markH * MARK_W) / MARK_H;
  const scale = markH / MARK_H; // px per mark unit
  const wm = WORDMARK[variant];
  const fontSize = wm.size * u;
  const gap = wm.gap * u;

  // --- The F -------------------------------------------------------------
  // Stem: rises out of the baseline along its own slant.
  const stemAt = (s: number) => 1 - ramp(s, T.stem[0], T.stem[1], EASE_OUT);
  const stemRest = stemAt(t);
  const stemV = (stemAt(t - 1 / fps) - stemRest) * MARK_H; // mark units / frame
  const stemD = MARK_H * 1.04;

  // Bars: flick out from behind the stem on a spring (a hair of overshoot).
  const bar = (start: number, travel: number) => {
    const at = (f: number) => spring({ frame: f - start * fps, fps, config: BAR_SPRING });
    const p = at(frame);
    return { x: -(1 - p) * travel, v: (p - at(frame - 1)) * travel };
  };
  const top = bar(T.top, 290);
  const mid = bar(T.mid, 250);
  const blurX = (v: number) => Math.min(Math.abs(v) * 0.32, 22);

  // --- Lockup ------------------------------------------------------------
  const slide = ramp(t, T.slide[0], T.slide[1]);
  const shift = (1 - slide) * (gap + word.width) / 2;
  const settle = 0.965 + 0.035 * ramp(t, 0, 1.7, EASE_OUT);
  const out = ramp(t, T.out[0], T.out[1], EASE_IN);
  const lockupScale = settle * (1 + 0.035 * out);

  // One specular band crosses the whole lockup (px from its left edge) and
  // switches the metal on: ahead of it the lockup sits a stop darker.
  const lockupW = markW + gap + word.width;
  const sweepP = ramp(t, T.sweep[0], T.sweep[1], Easing.bezier(0.45, 0, 0.3, 1));
  const bandW = 90 * u;
  const band = -2.2 * bandW + sweepP * (lockupW + 4.4 * bandW);

  const glow = ramp(t, 0.1, 1.3, EASE_OUT) * (1 - out);

  return (
    <AbsoluteFill style={{ background, overflow: "hidden" }}>
      <AbsoluteFill
        style={{
          opacity: glow,
          background: `radial-gradient(ellipse ${760 * u}px ${440 * u}px at 50% 50%, rgba(255,255,255,0.075), rgba(255,255,255,0.02) 45%, transparent 75%)`,
        }}
      />
      <div
        style={{
          position: "absolute",
          left: "50%",
          top: "50%",
          width: lockupW,
          height: markH,
          display: "flex",
          alignItems: "baseline",
          transform: `translate(-50%, -50%) translateX(${shift}px) scale(${lockupScale})`,
          opacity: 1 - out,
          filter: out > 0 ? `blur(${out * 6 * u}px)` : undefined,
        }}
      >
        <svg
          width={markW}
          height={markH}
          viewBox={`0 0 ${MARK_W} ${MARK_H}`}
          overflow="visible"
          style={{ flex: "none", overflow: "visible" }}
        >
          <defs>
            {(Object.keys(METAL) as (keyof typeof METAL)[]).map((k) => (
              <linearGradient key={k} id={`flick-${k}`} x1="0" y1="0" x2="0" y2="1">
                {METAL[k].map(([o, c]) => (
                  <stop key={o} offset={o} stopColor={c} />
                ))}
              </linearGradient>
            ))}
            <linearGradient
              id="flick-sweep"
              gradientUnits="userSpaceOnUse"
              x1={(band - 2 * bandW) / scale}
              x2={(band + 2 * bandW) / scale}
              y1={0}
              y2={0}
              gradientTransform={`skewX(${-(Math.atan(SLANT) * 180) / Math.PI})`}
            >
              {LIGHT.map(([o, c, a]) => (
                <stop key={o} offset={o} stopColor={c} stopOpacity={a} />
              ))}
            </linearGradient>
            <clipPath id="flick-floor">
              <rect x={-400} y={-400} width={1400} height={MARK_H + 400} />
            </clipPath>
            <clipPath id="flick-behind-stem">
              <polygon points={BEHIND_STEM} />
            </clipPath>
            <MotionBlur id="flick-blur-stem" x={blurX(stemV) * SLANT} y={blurX(stemV)} />
            <MotionBlur id="flick-blur-top" x={blurX(top.v)} y={0} />
            <MotionBlur id="flick-blur-mid" x={blurX(mid.v)} y={0} />
          </defs>

          <g clipPath="url(#flick-floor)">
            <polygon
              points={STEM}
              fill="url(#flick-stem)"
              transform={`translate(${-SLANT * stemRest * stemD} ${stemRest * stemD})`}
              filter={stemV > 0.05 ? "url(#flick-blur-stem)" : undefined}
            />
          </g>
          <g clipPath="url(#flick-behind-stem)">
            <polygon
              points={TOP}
              fill="url(#flick-top)"
              transform={`translate(${top.x} 0)`}
              filter={Math.abs(top.v) > 0.05 ? "url(#flick-blur-top)" : undefined}
            />
            <polygon
              points={MID}
              fill="url(#flick-mid)"
              transform={`translate(${mid.x} 0)`}
              filter={Math.abs(mid.v) > 0.05 ? "url(#flick-blur-mid)" : undefined}
            />
          </g>
          {/* Unclipped on purpose: bars still hidden behind the stem are shaded where they will land. */}
          <g fill="url(#flick-sweep)">
            <polygon points={STEM} transform={`translate(${-SLANT * stemRest * stemD} ${stemRest * stemD})`} clipPath="url(#flick-floor)" />
            <g clipPath="url(#flick-behind-stem)">
              <polygon points={TOP} transform={`translate(${top.x} 0)`} />
              <polygon points={MID} transform={`translate(${mid.x} 0)`} />
            </g>
          </g>
        </svg>

        <span
          ref={wordRef}
          style={{
            flex: "none",
            marginLeft: gap,
            fontFamily: FONT,
            fontSize,
            fontWeight: wm.weight,
            letterSpacing: wm.tracking,
            lineHeight: 1,
            whiteSpace: "pre",
          }}
        >
          {Array.from(wm.text, (ch, i) => {
            const p = ramp(t, T.letters + i * 0.045, T.letters + i * 0.045 + 0.6, EASE_OUT);
            // A CSS gradient restarts in every letter's box: shifting it by
            // left·sin(angle) keeps one continuous band across the word.
            const s = (band - markW - gap - (word.lefts[i] ?? 0)) * Math.sin((WORD_ANGLE * Math.PI) / 180);
            const light = `linear-gradient(${WORD_ANGLE}deg, ${LIGHT.map(([o, c, a]) => `${rgba(c, a)} ${s + (o - 0.5) * 4 * bandW}px`).join(", ")})`;
            return (
              <span
                key={i}
                style={{
                  display: "inline-block",
                  opacity: p,
                  transform: `translateX(${-(1 - p) * 0.32 * fontSize}px) skewX(${-wm.skew}deg)`,
                  transformOrigin: "50% 78%", // about the baseline
                  filter: p < 1 ? `blur(${(1 - p) * 14 * u}px)` : undefined,
                  backgroundImage: `${light}, linear-gradient(180deg, #ffffff 22%, #e4e5e7 58%, #b3b5b9 92%)`,
                  WebkitBackgroundClip: "text",
                  backgroundClip: "text",
                  color: "transparent",
                }}
              >
                {ch}
              </span>
            );
          })}
        </span>
      </div>
    </AbsoluteFill>
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
const rgba = (hex: string, a: number) => (hex === "#ffffff" ? `rgba(255,255,255,${a})` : `rgba(0,0,0,${a})`);

const MotionBlur: React.FC<{ id: string; x: number; y: number }> = ({ id, x, y }) => (
  <filter id={id} x="-60%" y="-60%" width="220%" height="220%" colorInterpolationFilters="sRGB">
    <feGaussianBlur stdDeviation={`${x} ${y}`} />
  </filter>
);
