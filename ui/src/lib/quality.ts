// Streaming quality caps. A cap only matters when the file is heavier: the
// server then transcodes at that bitrate and scales the picture down to the
// resolution in brackets (Rust: `max_width_for_bitrate`). The pairs follow
// usual real-time encoder ladders (Apple HLS authoring spec, Plex/Jellyfin
// presets): 4K ≈ 25+ Mb/s, 1080p ≈ 6–20, 720p ≈ 3–4, 480p ≈ 1.5–2, 360p ≈ 1.
export type QualityTier = { bitrate: number | null; label: string; detail: string };

export const QUALITY_TIERS: QualityTier[] = [
  { bitrate: null, label: "Original", detail: "No limit" },
  { bitrate: 80_000_000, label: "80 Mb/s", detail: "4K high" },
  { bitrate: 40_000_000, label: "40 Mb/s", detail: "4K" },
  { bitrate: 20_000_000, label: "20 Mb/s", detail: "1080p high" },
  { bitrate: 12_000_000, label: "12 Mb/s", detail: "1080p" },
  { bitrate: 8_000_000, label: "8 Mb/s", detail: "1080p low" },
  { bitrate: 4_000_000, label: "4 Mb/s", detail: "720p" },
  { bitrate: 2_000_000, label: "2 Mb/s", detail: "480p" },
  { bitrate: 1_000_000, label: "1 Mb/s", detail: "360p" },
];

/** The tier for a stored cap; an unknown value from older settings shows as-is. */
export function qualityTier(bitrate: number | null): QualityTier {
  return QUALITY_TIERS.find((t) => t.bitrate === bitrate) ?? { bitrate, label: `${((bitrate ?? 0) / 1e6).toFixed(0)} Mb/s`, detail: "" };
}

export const qualityText = (t: QualityTier) => (t.bitrate === null ? t.label : `${t.label} (${t.detail})`);
