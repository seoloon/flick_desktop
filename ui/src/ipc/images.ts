// Artwork URLs for the `oneshot-img` protocol (served by app/src/images.rs).
// The reference is opaque; Rust resolves it to an authenticated server URL,
// so no token ever appears in the DOM.
import type { ImageRef } from "./bindings/ImageRef";
import type { ImageSize } from "./bindings/ImageSize";

// WebView2 (Windows) serves custom schemes as http://<scheme>.localhost.
const WINDOWS_STYLE = /Windows|Android/.test(navigator.userAgent);
const BASE = WINDOWS_STYLE ? "http://oneshot-img.localhost/" : "oneshot-img://localhost/";

export function imageUrl(image: ImageRef | null | undefined, size: ImageSize): string | undefined {
  if (!image) return undefined;
  const enc = encodeURIComponent;
  return `${BASE}${size}/${image.kind}/${enc(image.item)}/${enc(image.tag)}`;
}

/** A profile's picture (proxied by Rust; `key` changes when the picture does). */
export function avatarUrl(profile: string, key: string): string {
  return `${BASE}avatar/${encodeURIComponent(profile)}/${encodeURIComponent(key)}`;
}

/** A TMDB photo or poster (`path` as TMDB gives it: `/abc.jpg`), proxied by Rust. */
export function tmdbImageUrl(path: string, size: "w185" | "h632" | "w342"): string {
  return `${BASE}tmdb/${size}${path}`;
}
