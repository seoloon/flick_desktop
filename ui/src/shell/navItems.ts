// Navigation adapts to what the servers actually offer.
import { useQuery } from "@tanstack/react-query";
import { Clapperboard, Heart, House, LayoutGrid, type LucideIcon, Search, Settings, ShieldCheck, Tv } from "lucide-react";
import { useLocation } from "react-router";
import { api } from "@/ipc/api";

export type NavItem = { id: string; href: string; label: string; icon: LucideIcon; active: boolean };

/**
 * Which nav entry a page belongs to. A detail page has no section of its own:
 * it stays in the one it was opened from (carried in the navigation state),
 * and belongs to Home when opened directly.
 */
export function navSection(pathname: string, search: string, state?: unknown): string {
  if (pathname.startsWith("/item/")) {
    const from = (state as { navSection?: unknown } | null)?.navSection;
    return typeof from === "string" ? from : "home";
  }
  const kind = new URLSearchParams(search).get("kind");
  if (pathname.startsWith("/libraries") || pathname.startsWith("/library/")) return kind === "movies" ? "movies" : kind === "shows" ? "shows" : "libraries";
  if (pathname === "/") return "home";
  if (pathname.startsWith("/favorites")) return "favorites";
  if (pathname.startsWith("/search")) return "search";
  if (pathname.startsWith("/admin")) return "admin";
  if (pathname.startsWith("/settings") || pathname.startsWith("/debug") || pathname.startsWith("/servers")) return "settings";
  return "";
}

export const librariesQuery = { queryKey: ["libraries"], queryFn: () => api.libraries() };
export const serversQuery = { queryKey: ["servers"], queryFn: () => api.serversList() };

export function useNavItems(): NavItem[] {
  const { pathname, search, state } = useLocation();
  const section = navSection(pathname, search, state);
  const libs = useQuery(librariesQuery);
  const servers = useQuery(serversQuery);
  const hasKind = (kind: string) => !!libs.data?.data.some((s) => s.libraries.some((l) => l.kind === kind));
  const hasAdmin = !!servers.data?.some((s) => s.connected && s.server.user.isAdmin);
  // Jellyfin favourites, or the Plex user's plex.tv Watchlist.
  const hasFavorites = !!servers.data?.some((s) => s.connected && !s.server.disabled);

  const items: (Omit<NavItem, "active"> & { show?: boolean })[] = [
    { id: "home", href: "/", label: "Home", icon: House },
    { id: "movies", href: "/libraries?kind=movies", label: "Movies", icon: Clapperboard, show: hasKind("movies") },
    { id: "shows", href: "/libraries?kind=shows", label: "TV Shows", icon: Tv, show: hasKind("shows") },
    { id: "libraries", href: "/libraries", label: "Libraries", icon: LayoutGrid },
    { id: "favorites", href: "/favorites", label: "Favourites", icon: Heart, show: hasFavorites },
    { id: "search", href: "/search", label: "Search", icon: Search },
    { id: "admin", href: "/admin", label: "Admin", icon: ShieldCheck, show: hasAdmin },
    { id: "settings", href: "/settings", label: "Settings", icon: Settings },
  ];
  return items.filter((i) => i.show !== false).map(({ show: _show, ...i }) => ({ ...i, active: i.id === section }));
}
