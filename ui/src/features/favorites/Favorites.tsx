// Favourites of the active profile on every server that has them (Jellyfin;
// Plex has none on library items), one shelf per kind, newest first.
import { useQuery } from "@tanstack/react-query";
import { Heart } from "lucide-react";
import { useEffect } from "react";
import { CenteredSpinner, EmptyState, Notice } from "@/components/tv/Feedback";
import { PageHeader } from "@/components/tv/Page";
import { Shelf } from "@/components/tv/Shelf";
import { ambientFor } from "@/lib/ambient";
import { favoriteShelves, favoritesQuery } from "@/lib/favorites";
import { Screen } from "@/nav/Focusable";

export function Favorites() {
  const favorites = useQuery(favoritesQuery);
  const items = favorites.data?.data ?? [];
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => ambientFor(items[0]), [favorites.data]);

  if (!favorites.data) return <CenteredSpinner />;
  if (items.length === 0) {
    return (
      <Screen>
        <EmptyState title="No favourites yet" icon={<Heart className="size-14 text-white/70" />}>
          Add titles with the heart on their page. Favourites follow your account on each Jellyfin server.
        </EmptyState>
      </Screen>
    );
  }

  return (
    <Screen ready className="pb-16">
      <div className="px-[var(--gutter)] pt-[var(--page-top)] pb-4">
        <PageHeader title="Favourites" />
      </div>
      {favorites.data.issues.length > 0 && (
        <div className="px-[var(--gutter)] pb-4">
          <Notice tone="warn">Some servers did not answer: {favorites.data.issues.map((i) => i.name).join(", ")}</Notice>
        </div>
      )}
      {favoriteShelves(items).map((s, i) => (
        <Shelf key={s.title} id={`favorites-${s.title}`} index={i} title={s.title} items={s.items} shape={s.shape} />
      ))}
    </Screen>
  );
}
