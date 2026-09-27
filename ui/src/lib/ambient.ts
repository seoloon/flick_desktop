// Adaptive background: the focused/selected item's artwork drives the whole
// screen. The blurred image comes straight from the artwork; the palette
// (computed in Rust, luminance-clamped so white text stays readable) tints it.
// Palette requests are debounced and cached so fast D-pad scrolling does not
// hammer the server.
import { create } from "zustand";
import { api } from "@/ipc/api";
import type { Palette } from "@/ipc/app-types";
import type { ImageRef } from "@/ipc/bindings/ImageRef";
import type { MediaItem } from "@/ipc/bindings/MediaItem";

type AmbientState = {
  /** Artwork painted behind everything, blurred. */
  image: ImageRef | null;
  palette: Palette | null;
  /** The item the user is looking at (focus/hover). */
  item: MediaItem | null;
};

export const useAmbient = create<AmbientState>(() => ({ image: null, palette: null, item: null }));

const cache = new Map<string, Palette>();
let timer: number | undefined;
let wanted = "";

const key = (img: ImageRef) => `${img.item}|${img.tag}`;

function apply(image: ImageRef, p: Palette | null) {
  useAmbient.setState({ image, palette: p });
  if (!p) return;
  const root = document.documentElement.style;
  root.setProperty("--ambient-base", p.base);
  root.setProperty("--ambient-accent", p.accent);
}

export function artworkFor(item: MediaItem | null | undefined): ImageRef | null {
  return item?.images.backdrop ?? item?.images.thumb ?? item?.images.poster ?? null;
}

/** Makes `item` the ambience. Cheap to call on every focus change. */
export function ambientFor(item: MediaItem | null | undefined) {
  if (!item) return;
  if (useAmbient.getState().item?.id !== item.id) useAmbient.setState({ item });
  const img = artworkFor(item);
  if (!img) return;
  const k = key(img);
  if (k === wanted) return;
  wanted = k;
  window.clearTimeout(timer);
  const hit = cache.get(k);
  if (hit) return apply(img, hit);
  // Debounced: while scrolling fast only the item the user stops on loads.
  timer = window.setTimeout(async () => {
    try {
      const p = await api.palette(img.item, img.kind, img.tag);
      cache.set(k, p);
      if (wanted === k) apply(img, p);
    } catch {
      // Palette unavailable: still show the artwork, untinted.
      if (wanted === k) apply(img, null);
    }
  }, 180);
}

/** No artwork, just soft light in `color` (the profile picker). The base
 * stays dark so white text keeps its contrast. */
export function ambientColor(color: string) {
  wanted = "";
  window.clearTimeout(timer);
  const base = `color-mix(in srgb, ${color} 12%, black)`;
  useAmbient.setState({ image: null, item: null, palette: { colors: [color, color], base, accent: color } });
  const root = document.documentElement.style;
  root.setProperty("--ambient-base", base);
  root.setProperty("--ambient-accent", color);
}

/** Forget the last artwork (a profile switch: nothing of the previous one stays). */
export function ambientReset() {
  wanted = "";
  window.clearTimeout(timer);
  useAmbient.setState({ image: null, palette: null, item: null });
}
