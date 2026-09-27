// Navigation adapts to what the servers actually offer.
import { useQuery } from "@tanstack/react-query";
import { Clapperboard, Heart, House, LayoutGrid, type LucideIcon, Search, Settings, ShieldCheck, Tv } from "lucide-react";
import { useLocation } from "react-router";
import { api } from "@/ipc/api";

export type NavItem = { id: string; href: string; label: string; icon: LucideIcon; active: boolean };

export const librariesQuery = { queryKey: ["libraries"], queryFn: () => api.libraries() };
export const serversQuery = { queryKey: ["servers"], queryFn: () => api.serversList() };

export function useNavItems(): NavItem[] {
  const { pathname, search } = useLocation();
  const libs = useQuery(librariesQuery);
  const servers = useQuery(serversQuery);
  const hasKind = (kind: string) => !!libs.data?.data.some((s) => s.libraries.some((l) => l.kind === kind));
  const hasAdmin = !!servers.data?.some((s) => s.connected && s.server.user.isAdmin);
  // Only Jellyfin keeps favourites on library items.
  const hasFavorites = !!servers.data?.some((s) => s.connected && !s.server.disabled && s.server.kind === "jellyfin");
  const kind = new URLSearchParams(search).get("kind");
  const inLibraries = pathname.startsWith("/libraries") || pathname.startsWith("/library/");

  const items: (Omit<NavItem, "active"> & { show?: boolean; match: boolean })[] = [
    { id: "home", href: "/", label: "Home", icon: House, match: pathname === "/" || pathname.startsWith("/item/") },
    { id: "movies", href: "/libraries?kind=movies", label: "Movies", icon: Clapperboard, show: hasKind("movies"), match: inLibraries && kind === "movies" },
    { id: "shows", href: "/libraries?kind=shows", label: "TV Shows", icon: Tv, show: hasKind("shows"), match: inLibraries && kind === "shows" },
    { id: "libraries", href: "/libraries", label: "Libraries", icon: LayoutGrid, match: inLibraries && kind !== "movies" && kind !== "shows" },
    { id: "favorites", href: "/favorites", label: "Favourites", icon: Heart, show: hasFavorites, match: pathname.startsWith("/favorites") },
    { id: "search", href: "/search", label: "Search", icon: Search, match: pathname.startsWith("/search") },
    { id: "admin", href: "/admin", label: "Admin", icon: ShieldCheck, show: hasAdmin, match: pathname.startsWith("/admin") },
    { id: "settings", href: "/settings", label: "Settings", icon: Settings, match: pathname.startsWith("/settings") || pathname.startsWith("/debug") || pathname.startsWith("/servers") },
  ];
  return items.filter((i) => i.show !== false).map(({ match, show: _show, ...i }) => ({ ...i, active: match }));
}
