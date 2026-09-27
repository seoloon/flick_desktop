// A title of a person's filmography that is not on the servers: TMDB
// poster, title, year and role. Focusable like a card, nothing to open.
import { motion } from "motion/react";
import type { KnownFor } from "@/ipc/bindings/KnownFor";
import { tmdbImageUrl } from "@/ipc/images";
import { focusSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { useTv } from "@/nav/Focusable";

export function KnownForCard({ credit, focusKey }: { credit: KnownFor; focusKey: string }) {
  const tv = useTv<HTMLDivElement>({ focusKey });
  return (
    <motion.div
      ref={tv.ref}
      {...tv.props}
      tabIndex={0}
      aria-label={`${credit.title}${credit.year ? ` (${credit.year})` : ""}, not on your servers`}
      animate={{ scale: tv.showFocus ? 1.08 : 1 }}
      transition={focusSpring}
      className="flex w-[var(--poster-w)] shrink-0 flex-col gap-3 outline-none"
    >
      <div className={cn("relative aspect-[2/3] w-full overflow-hidden rounded-lg bg-white/[0.06] shadow-[0_12px_30px_-16px_rgb(0_0_0/0.7)]", tv.showFocus && "ring-2 ring-white")}>
        {credit.poster ? (
          <img src={tmdbImageUrl(credit.poster, "w342")} alt="" loading="lazy" decoding="async" className="size-full object-cover opacity-80" />
        ) : (
          <span className="grid size-full place-items-center text-3xl font-semibold text-white/40">{credit.title.slice(0, 1)}</span>
        )}
      </div>
      <div className="flex min-w-0 flex-col">
        <span className="truncate text-[0.9375rem] font-semibold text-white/85">{credit.title}</span>
        <span className="truncate text-[0.8125rem] text-white/50">{[credit.year, credit.role].filter(Boolean).join(" · ")}</span>
      </div>
    </motion.div>
  );
}
