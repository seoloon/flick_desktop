// Subtitle fonts. mpv (libass) resolves a family name through the system
// (DirectWrite, CoreText, fontconfig), and so does the WebView: a family the
// WebView can render is one libass will find. Offering only installed fonts
// keeps the setting honest; the list favours clean sans faces that read well
// over video, like the streaming services' own subtitle fonts.
const CANDIDATES = [
  "Arial",
  // Not bare "Helvetica": Windows aliases it to Arial, so it would show twice.
  "Helvetica Neue",
  "SF Pro Display",
  "Segoe UI",
  "Roboto",
  "Inter",
  "Open Sans",
  "Noto Sans",
  "Source Sans 3",
  "Lato",
  "Montserrat",
  "Avenir Next",
  "Verdana",
  "Tahoma",
  "Trebuchet MS",
  "Calibri",
  "Bahnschrift",
  "DejaVu Sans",
  "Liberation Sans",
  "Ubuntu",
  "Georgia",
];

let cache: string[] | null = null;

/** True when `family` renders differently from both generic fallbacks. */
function installed(ctx: CanvasRenderingContext2D, family: string): boolean {
  const sample = "mmmmmmmmmmlli WQ@#0";
  return ["monospace", "serif"].some((fallback) => {
    ctx.font = `72px ${fallback}`;
    const base = ctx.measureText(sample).width;
    ctx.font = `72px "${family}", ${fallback}`;
    return ctx.measureText(sample).width !== base;
  });
}

export function installedSubtitleFonts(): string[] {
  if (cache) return cache;
  const ctx = document.createElement("canvas").getContext("2d");
  cache = ctx ? CANDIDATES.filter((f) => installed(ctx, f)) : [];
  return cache;
}

/** What an empty setting means, per platform (mirrors `subtitle_font` in Rust). */
export function defaultSubtitleFont(): string {
  if (/Mac/.test(navigator.userAgent)) return "Helvetica Neue";
  if (/Windows/.test(navigator.userAgent)) return "Arial";
  return "sans-serif";
}
