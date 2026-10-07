import { QueryClientProvider } from "@tanstack/react-query";
import { LayoutGroup, MotionConfig } from "motion/react";
import { lazy, type ReactNode, Suspense, useEffect, useState } from "react";
import { BrowserRouter, Navigate, Route, Routes, useLocation, useNavigate, useSearchParams } from "react-router";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { ItemMenu } from "@/components/tv/ItemMenu";
import { Toaster } from "@/components/ui/sonner";
import { LaunchIntro } from "@/features/intro/LaunchIntro";
import { useIntro } from "@/lib/intro";
import { Home } from "@/features/home/Home";
import { ProfilePicker } from "@/features/profiles/ProfilePicker";
import { ProfileGate } from "@/features/profiles/ProfileGate";
import { UpdatePrompt } from "@/features/updates/UpdatePrompt";
import { DownloadEvents } from "@/features/downloads/DownloadEvents";
import { WatchEvents } from "@/features/watch/WatchEvents";
import { toggleFrame } from "@/lib/mode";
import { useSettings } from "@/lib/settings";
import { goBack } from "@/lib/history";
import { onAction, setBackFallback } from "@/nav/input";
import { Shell } from "@/shell/Shell";
import { queryClient } from "@/lib/queryClient";

// Screens load on demand (the first paint only needs Home); they are all
// fetched in the background once the app is idle, so navigating stays instant.
const loaders = {
  Admin: () => import("@/features/admin/Admin").then((m) => ({ default: m.Admin })),
  Debug: () => import("@/features/debug/Debug").then((m) => ({ default: m.Debug })),
  Detail: () => import("@/features/detail/Detail").then((m) => ({ default: m.Detail })),
  Favorites: () => import("@/features/favorites/Favorites").then((m) => ({ default: m.Favorites })),
  PersonPage: () => import("@/features/person/PersonPage").then((m) => ({ default: m.PersonPage })),
  Libraries: () => import("@/features/library/Libraries").then((m) => ({ default: m.Libraries })),
  LibraryGrid: () => import("@/features/library/LibraryGrid").then((m) => ({ default: m.LibraryGrid })),
  PlayerView: () => import("@/features/player/PlayerView").then((m) => ({ default: m.PlayerView })),
  GenreGrid: () => import("@/features/search/GenreGrid").then((m) => ({ default: m.GenreGrid })),
  Search: () => import("@/features/search/Search").then((m) => ({ default: m.Search })),
  Settings: () => import("@/features/settings/Settings").then((m) => ({ default: m.Settings })),
  Watch: () => import("@/features/watch/Watch").then((m) => ({ default: m.Watch })),
};
const Admin = lazy(loaders.Admin);
const Debug = lazy(loaders.Debug);
const Detail = lazy(loaders.Detail);
const Favorites = lazy(loaders.Favorites);
const PersonPage = lazy(loaders.PersonPage);
const Libraries = lazy(loaders.Libraries);
const LibraryGrid = lazy(loaders.LibraryGrid);
const PlayerView = lazy(loaders.PlayerView);
const GenreGrid = lazy(loaders.GenreGrid);
const Search = lazy(loaders.Search);
const Settings = lazy(loaders.Settings);
const Watch = lazy(loaders.Watch);

function preloadScreens() {
  const run = () => Object.values(loaders).forEach((load) => void load());
  if ("requestIdleCallback" in window) requestIdleCallback(run, { timeout: 3000 });
  else setTimeout(run, 1500);
}

/** A new item remounts the player (fresh session state). */
function PlayerRoute() {
  const [params] = useSearchParams();
  const item = params.get("item");
  if (!item) return null;
  // Prerolls play first: `main` is the title they lead to, `rest` the clips still to come.
  const main = params.get("main");
  const preroll = main ? { main, rest: (params.get("rest") ?? "").split(",").filter(Boolean) } : undefined;
  return <PlayerView key={`${item}:${params.get("start") ?? ""}`} itemId={item} startMs={Number(params.get("start") ?? 0) || 0} lookForPrerolls={params.get("pre") !== "0"} preroll={preroll} resumeCast={params.get("cast") === "1"} />;
}

/** A render error shows a message instead of a blank window; navigating resets it. */
function Guarded({ children }: { children: ReactNode }) {
  const { key } = useLocation();
  return (
    <ErrorBoundary area="screen" resetKey={key}>
      {children}
    </ErrorBoundary>
  );
}

/** App-wide actions: Back walks history, Menu toggles Flick Frame. */
function GlobalActions() {
  const navigate = useNavigate();
  useEffect(() => {
    setBackFallback(() => goBack(navigate));
    return onAction((a) => {
      if (a.type !== "menu") return false;
      void toggleFrame();
      return true;
    });
  }, [navigate]);
  return null;
}

export function App() {
  const intensity = useSettings()?.appearance.animationIntensity ?? 1;
  // At launch, and each time playIntro() asks (entering Flick Frame); the app
  // runs underneath and loads while it plays.
  const run = useIntro((s) => s.run);
  const [shown, setShown] = useState(-1);
  useEffect(preloadScreens, []);
  return (
    <QueryClientProvider client={queryClient}>
      <MotionConfig reducedMotion={intensity === 0 ? "always" : "user"}>
        <BrowserRouter>
          <GlobalActions />
          <WatchEvents />
          <DownloadEvents />
          <ItemMenu />
          <ProfileGate />
          <UpdatePrompt launchReady={shown >= 0} />
          {/* One layout group: a profile's avatar flies from the picker to the sidebar. */}
          <LayoutGroup>
            <Guarded>
            <Suspense fallback={null}>
            <Routes>
              <Route path="/play" element={<PlayerRoute />} />
              <Route path="/profiles" element={<ProfilePicker />} />
              <Route element={<Shell />}>
                <Route path="/" element={<Home />} />
                <Route path="/libraries" element={<Libraries />} />
                <Route path="/library/:id" element={<LibraryGrid />} />
                <Route path="/item/:id" element={<Detail />} />
                <Route path="/search" element={<Search />} />
                <Route path="/search/genre" element={<GenreGrid />} />
                {/* Servers moved into Settings; old links keep working. */}
                <Route path="/servers" element={<Navigate to="/settings?s=servers" replace />} />
                <Route path="/favorites" element={<Favorites />} />
                <Route path="/watch" element={<Watch />} />
                <Route path="/person/:ref" element={<PersonPage />} />
                <Route path="/settings" element={<Settings />} />
                <Route path="/admin" element={<Admin />} />
                <Route path="/debug" element={<Debug />} />
              </Route>
            </Routes>
            </Suspense>
            </Guarded>
          </LayoutGroup>
        </BrowserRouter>
        <Toaster position="bottom-center" />
        {shown !== run && <LaunchIntro key={run} skip={intensity === 0} onDone={() => setShown(run)} />}
      </MotionConfig>
    </QueryClientProvider>
  );
}
