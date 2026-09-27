import { useQuery } from "@tanstack/react-query";
import { Clapperboard, LayoutGrid, type LucideIcon, Tv } from "lucide-react";
import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { Artwork } from "@/components/tv/Card";
import { CenteredSpinner, Notice } from "@/components/tv/Feedback";
import { Page, PageHeader } from "@/components/tv/Page";
import type { Library } from "@/ipc/bindings/Library";
import { focusSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { FocusGroup, Screen, useTv } from "@/nav/Focusable";
import { librariesQuery } from "@/shell/navItems";

const kindLabel: Record<string, string> = {
  movies: "Movies",
  shows: "TV Shows",
  music: "Music",
  photos: "Photos",
  musicVideos: "Music Videos",
  homeVideos: "Home Videos",
  mixed: "Mixed",
  liveTv: "Live TV",
  other: "Other",
};

const kindIcon: Record<string, LucideIcon> = { movies: Clapperboard, shows: Tv };

export function libraryPath(l: Library) {
  return `/library/${encodeURIComponent(l.id)}?kind=${l.kind}&name=${encodeURIComponent(l.name)}`;
}

/** Libraries without artwork: a quiet glyph on glass instead of a void. */
function FallbackArt({ kind }: { kind: string }) {
  const Icon = kindIcon[kind] ?? LayoutGrid;
  return (
    <div className="glass absolute inset-0 grid place-items-center">
      <Icon className="-mt-8 size-14 text-white/25" strokeWidth={1.4} />
    </div>
  );
}

function LibraryTile({ lib }: { lib: Library }) {
  const navigate = useNavigate();
  const tv = useTv<HTMLButtonElement>({ focusKey: `lib:${lib.id}` });
  const [hover, setHover] = useState(false);
  const lifted = tv.showFocus || hover;
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      onClick={() => navigate(libraryPath(lib))}
      onPointerEnter={() => setHover(true)}
      onPointerLeave={() => setHover(false)}
      animate={{ scale: tv.showFocus ? 1.07 : hover ? 1.03 : 1 }}
      whileTap={{ scale: 0.97 }}
      transition={focusSpring}
      className="relative aspect-video w-full cursor-pointer overflow-hidden rounded-2xl text-left scroll-my-24"
    >
      <motion.div
        className="absolute inset-0 overflow-hidden rounded-2xl"
        animate={{ boxShadow: lifted ? "0 30px 60px -20px rgb(0 0 0 / 0.85), 0 0 0 1px rgb(255 255 255 / 0.12)" : "0 10px 30px -18px rgb(0 0 0 / 0.6), 0 0 0 1px rgb(255 255 255 / 0.06)" }}
        transition={focusSpring}
      >
        {lib.image ? (
          <Artwork image={lib.image} size="large" alt="" />
        ) : (
          <FallbackArt kind={lib.kind} />
        )}
        <div className="absolute inset-0 bg-[linear-gradient(to_top,rgb(0_0_0/0.75),rgb(0_0_0/0.1)_60%)]" />
      </motion.div>
      <span className="absolute inset-x-5 bottom-4 flex flex-col">
        <span className="text-xl font-bold tracking-tight">{lib.name}</span>
        <span className="text-sm text-white/70">
          {kindLabel[lib.kind]}
          {lib.itemCount != null && ` · ${lib.itemCount} titles`}
        </span>
      </span>
    </motion.button>
  );
}

export function Libraries() {
  const [params] = useSearchParams();
  const kind = params.get("kind");
  const navigate = useNavigate();
  const data = useQuery(librariesQuery);
  const groups = (data.data?.data ?? [])
    .map((s) => ({ ...s, libraries: s.libraries.filter((l) => !kind || l.kind === kind) }))
    .filter((s) => s.libraries.length);

  // "Movies" with a single movie library goes straight to it.
  const only = kind && groups.length === 1 && groups[0]!.libraries.length === 1 ? groups[0]!.libraries[0] : undefined;
  useEffect(() => {
    if (only) navigate(libraryPath(only), { replace: true });
  }, [only, navigate]);

  return (
    <Screen ready={!!data.data}>
      <Page>
        <PageHeader title={kind ? (kindLabel[kind] ?? "Libraries") : "Libraries"} />
        {!data.data ? (
          <CenteredSpinner />
        ) : (
          <>
            {data.data.issues.length > 0 && <Notice tone="warn">{data.data.issues.map((i) => `${i.name}: ${i.error}`).join(" · ")}</Notice>}
            {groups.length === 0 && <p className="text-muted-foreground">No libraries yet. Connect a server in Servers.</p>}
            {groups.map((group) => (
              <section key={group.server.id} className="flex flex-col gap-4">
                {(groups.length > 1 || !kind) && <h2 className="text-[1.3125rem] font-semibold">{group.server.name}</h2>}
                <FocusGroup className={cn("grid gap-[var(--card-gap)]", "grid-cols-[repeat(auto-fill,minmax(19rem,1fr))]")}>
                  {group.libraries.map((lib) => (
                    <LibraryTile key={lib.id} lib={lib} />
                  ))}
                </FocusGroup>
              </section>
            ))}
          </>
        )}
      </Page>
    </Screen>
  );
}
