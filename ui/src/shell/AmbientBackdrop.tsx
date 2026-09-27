// The screen's only source of colour: the selected item's artwork fills the
// window, heavily blurred and darkened, with its palette as soft light.
// Settings > Appearance > "Artwork colour in background" scales it.
import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";
import { imageUrl } from "@/ipc/images";
import { useAmbient } from "@/lib/ambient";
import { ambientFade } from "@/lib/motion";

function BlurredArt({ src }: { src: string }) {
  const [loaded, setLoaded] = useState(false);
  return (
    <motion.img
      src={src}
      alt=""
      decoding="async"
      onLoad={() => setLoaded(true)}
      initial={{ opacity: 0, scale: 1.25 }}
      animate={loaded ? { opacity: 1, scale: 1.15 } : { opacity: 0, scale: 1.25 }}
      exit={{ opacity: 0 }}
      transition={ambientFade}
      className="absolute inset-0 size-full object-cover"
      style={{ filter: "blur(72px) saturate(1.6) brightness(0.62)" }}
    />
  );
}

export function AmbientBackdrop() {
  const image = useAmbient((s) => s.image);
  const palette = useAmbient((s) => s.palette);
  // A small rendition is plenty once blurred, and cheap to decode.
  const src = imageUrl(image, "card");
  const [c1, c2] = palette?.colors ?? [];

  return (
    <div aria-hidden className="pointer-events-none fixed inset-0 overflow-hidden bg-background">
      <div className="absolute inset-0" style={{ opacity: "calc(0.35 + var(--ambient-strength) * 0.65)" }}>
        <AnimatePresence initial={false}>{src && <BlurredArt key={src} src={src} />}</AnimatePresence>
      </div>
      {/* Palette light: two soft sources, crossfaded by CSS. */}
      <div
        className="absolute inset-0 transition-[background] duration-[1200ms]"
        style={{
          opacity: "var(--ambient-strength)",
          background: palette
            ? `radial-gradient(60% 55% at 85% 8%, color-mix(in srgb, ${c1 ?? palette.accent} 30%, transparent), transparent 70%), radial-gradient(55% 60% at 5% 100%, color-mix(in srgb, ${c2 ?? palette.base} 26%, transparent), transparent 70%)`
            : "none",
        }}
      />
      {/* Keeps white text readable whatever the artwork. */}
      <div className="absolute inset-0 bg-[linear-gradient(to_bottom,rgb(0_0_0/0.25),rgb(0_0_0/0.45)_60%,rgb(0_0_0/0.7))]" />
      <div className="absolute inset-0 bg-[radial-gradient(120%_90%_at_50%_30%,transparent_50%,rgb(0_0_0/0.55))]" />
    </div>
  );
}
