// Geometry of the flat mark (`flick-mark.svg`), in its own 488.64 × 480 space.
export const MARK_W = 488.64;
export const MARK_H = 480;

// Every edge of the F leans by the same italic: dx/dy = 84.64 / 480 (10°).
export const SLANT = 84.64 / 480;

export const STEM = "84.64,0 208.64,0 124,480 0,480";
export const TOP = "224.64,0 488.64,0 468.89,112 204.89,112";
// The middle bar carries a play-arrow notch on its left end.
export const MID = "192.19,184 408.19,184 377.86,356 161.86,356 323.23,270";

// The bars slide out from behind the stem: they are clipped to the right of
// the line halfway across the 16-unit gap, which runs parallel to the stem.
const lineX = (y: number) => 216.64 - SLANT * y;
export const BEHIND_STEM = `${lineX(-400)},-400 2000,-400 2000,880 ${lineX(880)},880`;

// Metal fills, the same ramps as the app icon (`flick-icon.svg`).
export const METAL = {
  stem: [
    [0, "#fbfbfc"],
    [0.4, "#d3d4d7"],
    [0.75, "#b0b2b6"],
    [1, "#8f9195"],
  ],
  top: [
    [0, "#ffffff"],
    [0.55, "#eeeff0"],
    [1, "#cfd0d3"],
  ],
  mid: [
    [0, "#ecedef"],
    [0.5, "#c9cacd"],
    [1, "#a2a4a8"],
  ],
} as const;
