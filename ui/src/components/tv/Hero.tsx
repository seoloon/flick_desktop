// Pieces shared by the Home carousel and the detail page: full-bleed artwork
// melting into the ambient backdrop, the title logo, and the facts line.
import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, useState } from "react";
import ProgressiveBlur from "@/components/smoothui/progressive-blur";
import type { ImageRef } from "@/ipc/bindings/ImageRef";
import type { MediaItem } from "@/ipc/bindings/MediaItem";
import { imageUrl } from "@/ipc/images";
import { duration, episodeLabel } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Pill } from "./Page";

function FadeImage({ src }: { src: string }) {
  const [loaded, setLoaded] = useState(false);
  return (
    <motion.img
      src={src}
      alt=""
      decoding="async"
      onLoad={() => setLoaded(true)}
      initial={{ opacity: 0, scale: 1.04 }}
      animate={loaded ? { opacity: 1, scale: 1 } : { opacity: 0, scale: 1.04 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.9, ease: [0.32, 0.72, 0, 1] }}
      className="absolute inset-0 size-full object-cover object-[center_20%]"
    />
  );
}

/**
 * Sharp artwork behind a hero, bleeding under the sidebar. It fades out at
 * the bottom through a progressive blur, so the shelves below sit on the
 * blurred ambient rather than on a hard edge.
 */
export function HeroBackdrop({ image, className }: { image: ImageRef | null | undefined; className?: string }) {
  const src = imageUrl(image, "hero");
  return (
    // `isolate` keeps the blur layers' z-indexes below the hero content; the
    // mask fades every layer out together, so there is no seam at the bottom.
    <div
      aria-hidden
      className={cn(
        "absolute inset-0 isolate -ml-[var(--content-left)] overflow-hidden [mask-image:linear-gradient(to_bottom,black_55%,transparent)]",
        className,
      )}
    >
      <AnimatePresence initial={false}>{src && <FadeImage key={src} src={src} />}</AnimatePresence>
      <ProgressiveBlur direction="bottom" blur={28} layers={7} fadeIn={false} className="top-auto h-[40%]" />
      {/* Legibility for the title block (bottom-left). */}
      <div className="absolute inset-0 bg-[linear-gradient(to_right,rgb(0_0_0/0.7),rgb(0_0_0/0.3)_38%,transparent_65%)]" />
      <div className="absolute inset-x-0 bottom-0 h-1/2 bg-[linear-gradient(to_top,rgb(0_0_0/0.4),transparent)]" />
    </div>
  );
}

/** Title treatment: the artwork's logo when the server has one. */
export function TitleArt({ item, className }: { item: MediaItem; className?: string }) {
  const logo = imageUrl(item.images.logo, "large");
  const [failed, setFailed] = useState(false);
  const title = item.episode?.seriesTitle ?? item.title;
  if (logo && !failed) {
    return (
      <img
        src={logo}
        alt={title}
        onError={() => setFailed(true)}
        className={cn("max-h-40 w-auto max-w-[min(34rem,70%)] object-contain object-left drop-shadow-[0_6px_24px_rgb(0_0_0/0.5)]", className)}
      />
    );
  }
  return <h1 className={cn("max-w-3xl text-[3.5rem] leading-[0.95] font-bold tracking-tight text-balance drop-shadow-[0_4px_24px_rgb(0_0_0/0.45)]", className)}>{title}</h1>;
}

export function MetaLine({ item, extra }: { item: MediaItem; extra?: ReactNode }) {
  const parts: ReactNode[] = [];
  if (item.episode) parts.push(`${episodeLabel(item)} · ${item.title}`);
  if (item.genres.length) parts.push(item.genres.slice(0, 2).join(", "));
  if (item.year) parts.push(item.year);
  if (item.runtimeMs) parts.push(duration(item.runtimeMs));
  return (
    <div className="flex flex-wrap items-center gap-x-2.5 gap-y-2 text-[0.9375rem] font-medium text-white/80">
      {parts.map((p, i) => (
        <span key={i} className="flex items-center gap-2.5">
          {i > 0 && <span className="text-white/35">·</span>}
          {p}
        </span>
      ))}
      {item.officialRating && <Pill>{item.officialRating}</Pill>}
      {extra}
    </div>
  );
}
