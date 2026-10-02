// A genre as a tile: the poster of a random title of that genre, darkened so
// the name stays legible. The poster is drawn once per session (not per render)
// and only fetched when the tile scrolls into view: a library with 25 genres
// must not ask its servers 25 questions at once.
import { useQuery } from "@tanstack/react-query";
import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { Artwork } from "@/components/tv/Card";
import { api } from "@/ipc/api";
import type { ImageRef } from "@/ipc/bindings/ImageRef";
import type { ItemKind } from "@/ipc/bindings/ItemKind";
import { focusSpring } from "@/lib/motion";
import { useTv } from "@/nav/Focusable";

/** Where a genre's titles are listed. */
export const genrePath = (kind: ItemKind, genre: string) => `/search/genre?kind=${kind}&name=${encodeURIComponent(genre)}`;

/** Titles asked for per cover: a few, so one without a poster does not leave the tile bare. */
const COVER_CANDIDATES = 8;

function useCover(kind: ItemKind, genre: string, enabled: boolean) {
  return useQuery({
    queryKey: ["genre-cover", kind, genre],
    enabled,
    staleTime: Infinity,
    gcTime: Infinity,
    queryFn: async (): Promise<ImageRef | null> => {
      const res = await api.byGenre({ kind, genre, sort: "random", order: "ascending", start: 0, limit: COVER_CANDIDATES });
      const posters = res.data.flatMap((i) => (i.images.poster ? [i.images.poster] : []));
      return posters[Math.floor(Math.random() * posters.length)] ?? null;
    },
  });
}

export function GenreTile({ kind, genre }: { kind: ItemKind; genre: string }) {
  const navigate = useNavigate();
  const tv = useTv<HTMLButtonElement>({ focusKey: `genre:${kind}:${genre}` });
  const [visible, setVisible] = useState(false);
  const [node, setNode] = useState<HTMLButtonElement | null>(null);
  const cover = useCover(kind, genre, visible);

  useEffect(() => {
    if (!node || visible) return;
    const io = new IntersectionObserver(([e]) => e?.isIntersecting && setVisible(true), { rootMargin: "300px" });
    io.observe(node);
    return () => io.disconnect();
  }, [node, visible]);

  return (
    <motion.button
      ref={(el) => {
        (tv.ref as { current: HTMLButtonElement | null }).current = el;
        setNode(el);
      }}
      type="button"
      {...tv.props}
      onClick={() => navigate(genrePath(kind, genre))}
      aria-label={genre}
      className="relative aspect-[16/10] w-full cursor-pointer overflow-hidden rounded-xl bg-white/[0.06] text-left scroll-mx-[var(--gutter)] scroll-mt-24 scroll-mb-28"
      animate={{
        scale: tv.showFocus ? 1.06 : 1,
        zIndex: tv.showFocus ? 10 : 0,
        boxShadow: tv.showFocus ? "0 32px 56px -18px rgb(0 0 0 / 0.85), 0 0 0 2px rgb(255 255 255 / 0.85)" : "0 10px 24px -14px rgb(0 0 0 / 0.6), 0 0 0 1px rgb(255 255 255 / 0.06)",
      }}
      whileTap={{ scale: tv.showFocus ? 1.02 : 0.97 }}
      transition={focusSpring}
    >
      {cover.data && <Artwork image={cover.data} size="large" alt="" className="[&_img]:object-[50%_30%]" />}
      {/* Darkening: flat enough to read on any poster, heavier at the bottom edge. */}
      <span aria-hidden className="absolute inset-0 bg-gradient-to-t from-black/75 via-black/50 to-black/40" />
      <span className="absolute inset-0 grid place-items-center p-4 text-center font-heading text-xl leading-tight font-bold tracking-tight text-balance text-white [text-shadow:0_1px_12px_rgb(0_0_0/0.6)]">
        {genre}
      </span>
    </motion.button>
  );
}
