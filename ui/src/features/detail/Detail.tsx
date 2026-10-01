// Title page: the artwork fills the top, the actions sit on it, then seasons
// or children, cast, technical details and similar titles.
import { useQuery } from "@tanstack/react-query";
import { Check, Heart, Play, RotateCcw, Users } from "lucide-react";
import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { MediaCard } from "@/components/tv/Card";
import { CenteredSpinner, EmptyState } from "@/components/tv/Feedback";
import { HeroBackdrop, MetaLine, TitleArt } from "@/components/tv/Hero";
import { BackButton } from "@/components/tv/BackButton";
import { Facts, Panel, Pill } from "@/components/tv/Page";
import { Segmented } from "@/components/tv/Segmented";
import { Shelf } from "@/components/tv/Shelf";
import { api, asError } from "@/ipc/api";
import type { Credit } from "@/ipc/bindings/Credit";
import type { ItemRef } from "@/ipc/bindings/ItemRef";
import type { MediaItem } from "@/ipc/bindings/MediaItem";
import { imageUrl } from "@/ipc/images";
import { ambientFor } from "@/lib/ambient";
import { setItemFlag, useOverridden } from "@/lib/itemMenu";
import { audioCodecLabel, badges, bitrate, channelsLabel, remaining, resolutionLabel, videoCodecLabel } from "@/lib/format";
import { enter, focusSpring } from "@/lib/motion";
import { FocusGroup, Screen, useTv } from "@/nav/Focusable";
import { ServerBadge } from "@/components/tv/ServerBadge";
import { personPath } from "@/lib/person";
import { useSources } from "@/lib/servers";
import { isPlayable, playPath } from "../player/route";
import { useWatch } from "../watch/store";

export function Detail() {
  const { id: raw = "" } = useParams();
  const id = decodeURIComponent(raw);
  const navigate = useNavigate();
  // Instant paint from cache, then the fresh server copy.
  const cached = useQuery({ queryKey: ["item-cached", id], queryFn: () => api.itemCached(id) });
  const fresh = useQuery({ queryKey: ["item", id], queryFn: () => api.item(id) });
  const similar = useQuery({ queryKey: ["similar", id], queryFn: () => api.similar(id).catch(() => []) });
  const loaded = fresh.data ?? cached.data ?? undefined;
  const item = useOverridden(loaded);
  const room = useWatch((w) => w.room);
  const watch = useWatch((w) => w.status);
  // The server that plays (the item's own reference) first, then copies elsewhere.
  const sources = useSources(item ? [item.id, ...(item.alternates ?? [])] : []);

  useEffect(() => ambientFor(item), [item]);

  const toggle = (what: "played" | "favorite") => {
    if (item) void setItemFlag(item, what).catch((e) => toast.error(asError(e).message));
  };

  if (!item) {
    return fresh.error ? (
      <Screen>
        <EmptyState title="Not available">{asError(fresh.error).message}</EmptyState>
      </Screen>
    ) : (
      <CenteredSpinner />
    );
  }

  const resuming = item.user.positionMs > 0;
  const source = item.sources[0];
  // In a watch room only the host picks the title; guests are told instead of getting an error screen.
  const start = (ms: number) => {
    if (room && room.hostId !== room.you) toast("Only the host can choose what the room watches.");
    else navigate(playPath(item.id, ms));
  };

  return (
    <Screen ready>
      <section className="relative flex min-h-[max(36rem,80vh)] flex-col justify-end">
        <HeroBackdrop image={item.images.backdrop ?? item.images.thumb} />
        <BackButton className="absolute top-[var(--page-top)] left-[var(--gutter)] z-10" />
        {/* The block slides; only its text fades. The glass buttons must not
            sit under a fading ancestor, or their blur switches on late. */}
        <motion.div initial={{ y: 20 }} animate={{ y: 0 }} transition={enter} className="relative flex max-w-3xl flex-col gap-5 px-[var(--gutter)] pb-14">
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={enter} className="flex flex-col gap-5">
            <TitleArt item={item} />
            {item.tagline && <p className="text-lg font-medium text-white/85 italic">{item.tagline}</p>}
            <MetaLine
              item={item}
              extra={
                <>
                  {item.communityRating != null && <span className="text-white/80">★ {item.communityRating.toFixed(1)}</span>}
                  {badges(source).map((b) => (
                    <Pill key={b}>{b}</Pill>
                  ))}
                </>
              }
            />
            {sources[0] && (
              <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
                <ServerBadge server={sources[0]} />
                {sources.length > 1 && (
                  <>
                    <span className="text-white/35">·</span>
                    <span className="text-white/50">Also on</span>
                    {sources.slice(1).map((s) => (
                      <ServerBadge key={s.id} server={s} quiet />
                    ))}
                  </>
                )}
              </div>
            )}
          </motion.div>
          <FocusGroup focusKey="detail-actions" className="flex flex-wrap items-center gap-3 pt-1">
            {isPlayable(item) && (
              <>
                <Button variant="primary" size="lg" icon={Play} iconFilled autoFocus onClick={() => start(resuming ? item.user.positionMs : 0)}>
                  {resuming ? `Resume · ${remaining(item)}` : "Play"}
                </Button>
                {resuming && <Button size="lg" icon={RotateCcw} label="Start over" onClick={() => start(0)}>Start Over</Button>}
                {(item.kind === "movie" || item.kind === "episode") && (room ? room.hostId === room.you : watch?.available) && (
                  <Button size="lg" icon={Users} onClick={() => (room ? void api.flicksyncSelectMedia(item.id).catch((e) => toast.error(asError(e).message)) : navigate("/watch"))}>
                    {room ? "Watch in Room" : "Watch Together"}
                  </Button>
                )}
              </>
            )}
            <Button size="icon-lg" icon={Check} label={item.user.played ? "Mark as unwatched" : "Mark as watched"} className={item.user.played ? "bg-white/25" : undefined} onClick={() => toggle("played")} />
            <Button size="icon-lg" icon={Heart} iconFilled={item.user.favorite} label={item.user.favorite ? "Remove from favourites" : "Add to favourites"} onClick={() => toggle("favorite")} />
          </FocusGroup>
          {item.overview && <motion.p initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ ...enter, delay: 0.1 }} className="line-clamp-4 max-w-2xl text-[1.0625rem] leading-relaxed text-white/80 text-pretty">{item.overview}</motion.p>}
        </motion.div>
      </section>

      <div className="relative flex flex-col gap-4 pb-20">
        {item.kind === "series" && <Seasons series={item} />}
        {(item.kind === "season" || item.kind === "collection" || item.kind === "playlist") && <Children item={item} />}
        {item.credits.length > 0 && <Cast credits={item.credits.slice(0, 30)} from={item.id} />}
        {similar.data && similar.data.length > 0 && <Shelf id="similar" title="More Like This" items={similar.data} shape="poster" />}
        <TechInfo item={item} />
      </div>
    </Screen>
  );
}

function Seasons({ series }: { series: MediaItem }) {
  const seasons = useQuery({ queryKey: ["children", series.id, "series"], queryFn: () => api.children(series.id, "series") });
  const [season, setSeason] = useState<string | null>(null);
  // Open the season of the next unwatched episode.
  const current = season ?? (seasons.data?.find((s) => (s.user.unplayedCount ?? 1) > 0) ?? seasons.data?.[0])?.id ?? null;
  const episodes = useQuery({
    queryKey: ["children", current, "season"],
    queryFn: () => api.children(current!, "season"),
    enabled: !!current,
  });

  return (
    <section className="flex flex-col gap-2">
      {seasons.data && seasons.data.length > 1 && (
        <div className="px-[var(--gutter)] pb-2">
          <Segmented options={seasons.data.map((s) => ({ value: s.id, label: s.title }))} value={current ?? ""} onChange={setSeason} label="Seasons" />
        </div>
      )}
      {episodes.data ? (
        <EpisodeShelf key={current} episodes={episodes.data} title={seasons.data?.length === 1 ? (seasons.data[0]?.title ?? "Episodes") : "Episodes"} />
      ) : (
        <div className="px-[var(--gutter)] py-10">
          <CenteredSpinner />
        </div>
      )}
    </section>
  );
}

function EpisodeShelf({ episodes, title }: { episodes: MediaItem[]; title: string }) {
  return <Shelf id="episodes" title={title} items={episodes} shape="thumb" />;
}

function Children({ item }: { item: MediaItem }) {
  const children = useQuery({ queryKey: ["children", item.id, item.kind], queryFn: () => api.children(item.id, item.kind) });
  if (!children.data) return <CenteredSpinner />;
  const season = item.kind === "season";
  return <Shelf id="children" title={season ? "Episodes" : "Titles"} items={children.data} shape={season ? "thumb" : "poster"} />;
}

function Cast({ credits, from }: { credits: Credit[]; from: ItemRef }) {
  return (
    <section className="flex flex-col">
      <h2 className="px-[var(--gutter)] text-[1.3125rem] font-semibold">Cast &amp; Crew</h2>
      <FocusGroup focusKey="cast" fade="x" arrows className="[--fade-size:var(--gutter)] no-scrollbar flex gap-6 overflow-x-auto px-[var(--gutter)] pt-5 pb-8">
        {credits.map((c, i) => (
          <Person key={`${c.name}-${i}`} credit={c} from={from} />
        ))}
      </FocusGroup>
    </section>
  );
}

function Person({ credit, from }: { credit: Credit; from: ItemRef }) {
  const navigate = useNavigate();
  const tv = useTv<HTMLButtonElement>();
  const src = imageUrl(credit.image, "tiny");
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      animate={{ scale: tv.showFocus ? 1.1 : 1 }}
      transition={focusSpring}
      onClick={() => navigate(personPath(credit, from))}
      className="flex w-28 shrink-0 cursor-pointer flex-col items-center gap-2.5 text-center scroll-mx-[var(--gutter)]"
    >
      <span className="relative size-24 overflow-hidden rounded-full bg-white/[0.08] shadow-[0_12px_30px_-14px_rgb(0_0_0/0.8)] ring-1 ring-white/10">
        {src ? (
          <img src={src} alt="" loading="lazy" className="size-full object-cover" />
        ) : (
          <span className="grid size-full place-items-center text-2xl font-semibold text-white/50">{credit.name.slice(0, 1)}</span>
        )}
      </span>
      <span className="flex flex-col">
        <span className="line-clamp-2 text-sm leading-tight font-semibold">{credit.name}</span>
        <span className="line-clamp-2 text-xs leading-tight text-muted-foreground">{credit.character ?? credit.role}</span>
      </span>
    </motion.button>
  );
}

function TechInfo({ item }: { item: MediaItem }) {
  const [index, setIndex] = useState("0");
  const s = item.sources[Number(index)];
  if (!item.sources.length || !s) return null;
  const rows: [string, string][] = [["File", `${s.container?.toUpperCase() ?? ""} ${bitrate(s.bitrate)}`.trim()]];
  for (const v of s.video) {
    const range = v.range.kind === "unknown" ? "" : v.range.kind.toUpperCase().replace("-", " ");
    const fps = v.frameRate ? `${v.frameRate.toFixed(3).replace(/\.?0+$/, "")} fps` : "";
    rows.push(["Video", [resolutionLabel(v.width, v.height), videoCodecLabel(v.codec), v.bitDepth ? `${v.bitDepth}-bit` : "", range, fps].filter(Boolean).join(" · ")]);
  }
  for (const a of s.audio) {
    const spatial = a.spatial === "dolby-atmos" ? "Atmos" : a.spatial === "dts-x" ? "DTS:X" : "";
    rows.push(["Audio", [a.language ?? "Unknown", audioCodecLabel(a.codec), channelsLabel(a.channels), spatial, a.title ?? ""].filter(Boolean).join(" · ")]);
  }
  for (const t of s.subtitles) {
    rows.push(["Subtitles", [t.language ?? "Unknown", t.format.toUpperCase(), t.forced ? "forced" : "", t.hearingImpaired ? "SDH" : "", t.external ? "external" : ""].filter(Boolean).join(" · ")]);
  }
  return (
    <div className="px-[var(--gutter)] pt-4">
      <Panel
        title="Media Info"
        className="max-w-4xl"
        actions={
          item.sources.length > 1 ? (
            <Segmented size="sm" options={item.sources.map((src, i) => ({ value: String(i), label: src.name ?? `Version ${i + 1}` }))} value={index} onChange={setIndex} label="Versions" />
          ) : undefined
        }
      >
        <Facts rows={rows} />
        <p className="text-xs text-muted-foreground">Reported by the server. The player verifies the real stream when playback starts.</p>
      </Panel>
    </div>
  );
}
