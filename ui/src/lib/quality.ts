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

// Display limit: client-side only. The file is downloaded as before; the
// picture is scaled down before it is drawn, to spare the GPU. Not a network
// cap (above). 2160 is the top setting and means no limit.
export const DISPLAY_TOP = 2160;
export const DISPLAY_TIERS = [
  { height: 360, label: "360p" },
  { height: 480, label: "480p" },
  { height: 720, label: "720p" },
  { height: 1080, label: "1080p" },
  { height: 1440, label: "1440p" },
  { height: DISPLAY_TOP, label: "4K" },
];

/** Tiers worth offering for a picture `sourceHeight` lines tall (all when unknown):
 *  never one the file cannot reach. The top entry stands for "as the file is". */
export function displayTiers(sourceHeight?: number | null): { height: number; label: string; detail?: string }[] {
  if (!sourceHeight) return DISPLAY_TIERS;
  const below = DISPLAY_TIERS.filter((t) => t.height < DISPLAY_TOP && t.height < sourceHeight);
  return [...below, { height: DISPLAY_TOP, label: "Maximum", detail: `As the file is · ${sourceHeight}p` }];
}

/** The tier a stored limit falls on for this picture (anything at or above it is "Maximum"). */
export function displaySelected(limit: number, sourceHeight?: number | null): number {
  return limit >= (sourceHeight ?? DISPLAY_TOP) ? DISPLAY_TOP : limit;
}

export const DISPLAY_NOTE = "Only changes how the picture is drawn on this computer, to ease the graphics card; the processor works more. It does not change what is downloaded: for that, use the network limit.";
