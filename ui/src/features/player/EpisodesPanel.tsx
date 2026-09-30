// Episodes of the show being watched, one season at a time, in the same dark
// card as the settings menu. Picking one switches playback to it.
import { useQuery } from "@tanstack/react-query";
import { Check } from "lucide-react";
import { motion } from "motion/react";
import { useState } from "react";
import { api } from "@/ipc/api";
import type { ItemRef } from "@/ipc/bindings/ItemRef";
import type { MediaItem } from "@/ipc/bindings/MediaItem";
import { imageUrl } from "@/ipc/images";
import { clock } from "@/lib/format";
import { focusSpring, panelSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";

function SeasonChip({ label, selected, onSelect }: { label: string; selected: boolean; onSelect: () => void }) {
  const tv = useTv<HTMLButtonElement>({ scroll: "nearest" });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      role="tab"
      aria-selected={selected}
      onClick={onSelect}
      animate={{ scale: tv.showFocus ? 1.04 : 1 }}
      transition={focusSpring}
      className={cn(
        "h-8 shrink-0 cursor-pointer rounded-full px-3.5 text-xs font-semibold whitespace-nowrap transition-colors",
        tv.showFocus ? "bg-white text-black" : selected ? "bg-white/20 text-white" : "text-white/60 hover:bg-white/10 hover:text-white",
      )}
    >
      {label}
    </motion.button>
  );
}

function EpisodeRow({ episode, current, onPick }: { episode: MediaItem; current: boolean; onPick: () => void }) {
  const tv = useTv<HTMLButtonElement>({ scroll: "nearest", autoFocus: current });
  const art = imageUrl(episode.images.thumb ?? episode.images.backdrop, "card");
  const progress = !episode.user.played && episode.runtimeMs ? Math.min(1, episode.user.positionMs / episode.runtimeMs) : 0;
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      onClick={onPick}
      aria-current={current ? "true" : undefined}
      data-tv-focus={tv.showFocus || undefined}
      animate={{ scale: tv.showFocus ? 1.02 : 1 }}
      transition={focusSpring}
      className={cn("group/row flex w-full cursor-pointer items-center gap-3 rounded-xl p-2 text-left transition-colors duration-200 scroll-my-3", tv.showFocus ? "bg-white text-black" : "hover:bg-white/10")}
    >
      <div className="relative aspect-video w-28 shrink-0 overflow-hidden rounded-lg bg-white/10">
        {art && <img src={art} alt="" loading="lazy" className="size-full object-cover" />}
        {progress > 0 && (
          <span className="absolute inset-x-0 bottom-0 h-1 bg-black/50">
            <span className="block h-full bg-white" style={{ width: `${progress * 100}%` }} />
          </span>
        )}
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className={cn("text-[0.6875rem] font-semibold tracking-wide uppercase", tv.showFocus ? "text-black/55" : "text-white/55")}>
          {current ? "Now playing" : episode.episode?.episodeNumber != null ? `Episode ${episode.episode.episodeNumber}` : "Episode"}
          {episode.runtimeMs ? ` · ${clock(episode.runtimeMs)}` : ""}
        </span>
        <span className="line-clamp-2 text-[0.8125rem] font-semibold">{episode.title}</span>
      </div>
      {episode.user.played && !current && <Check className="size-4 shrink-0 opacity-60" strokeWidth={3} aria-label="Watched" />}
    </motion.button>
  );
}

export function EpisodesPanel({ series, season, currentId, onPick }: { series: ItemRef; season: ItemRef | null; currentId: ItemRef; onPick: (id: ItemRef) => void }) {
  const seasons = useQuery({ queryKey: ["children", series, "series"], queryFn: () => api.children(series, "series") });
  const [chosen, setChosen] = useState<ItemRef | null>(null);
  const shown = chosen ?? season ?? seasons.data?.[0]?.id ?? null;
  const episodes = useQuery({ queryKey: ["children", shown, "season"], queryFn: () => api.children(shown!, "season"), enabled: !!shown });

  return (
    <motion.aside
      aria-label="Episodes"
      initial={{ opacity: 0, y: 12, scale: 0.94 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 8, scale: 0.96 }}
      transition={panelSpring}
      style={{ transformOrigin: "bottom right" }}
      className="absolute right-8 bottom-[calc(100%+0.25rem)] flex w-[min(28rem,calc(100vw-4rem))] flex-col gap-1 overflow-hidden rounded-2xl bg-black/75 p-1.5 text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.12),inset_0_0_0_1px_rgb(255_255_255/0.08),0_30px_80px_-20px_rgb(0_0_0/0.8)]"
    >
      <FocusGroup focusKey="player-episodes" boundary className="flex flex-col gap-1">
        {seasons.data && seasons.data.length > 1 && (
          <FocusGroup fade="x" role="tablist" aria-label="Seasons" className="[--fade-size:1rem] no-scrollbar flex gap-1 overflow-x-auto p-1">
            {seasons.data.map((s) => (
              <SeasonChip key={s.id} label={s.title} selected={s.id === shown} onSelect={() => setChosen(s.id)} />
            ))}
          </FocusGroup>
        )}
        <FocusGroup fade="y" className="[--fade-size:1.25rem] no-scrollbar -m-1 flex max-h-[45vh] flex-col gap-0.5 overflow-y-auto p-1">
          {episodes.data?.map((e) => <EpisodeRow key={`${shown}:${e.id}`} episode={e} current={e.id === currentId} onPick={() => onPick(e.id)} />)}
          {episodes.data && episodes.data.length === 0 && <p className="px-3 py-2 text-[0.8125rem] text-white/55">No episodes.</p>}
          {!episodes.data && <p className="px-3 py-2 text-[0.8125rem] text-white/55">Loading…</p>}
        </FocusGroup>
      </FocusGroup>
    </motion.aside>
  );
}
