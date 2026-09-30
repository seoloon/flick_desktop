import { QueryClientProvider } from "@tanstack/react-query";
import { LayoutGroup, MotionConfig } from "motion/react";
import { useEffect, useState } from "react";
import { BrowserRouter, Navigate, Route, Routes, useNavigate, useSearchParams } from "react-router";
import { ItemMenu } from "@/components/tv/ItemMenu";
import { Toaster } from "@/components/ui/sonner";
import { Admin } from "@/features/admin/Admin";
import { Debug } from "@/features/debug/Debug";
import { Detail } from "@/features/detail/Detail";
import { Favorites } from "@/features/favorites/Favorites";
import { LaunchIntro } from "@/features/intro/LaunchIntro";
import { useIntro } from "@/lib/intro";
import { PersonPage } from "@/features/person/PersonPage";
import { Home } from "@/features/home/Home";
import { Libraries } from "@/features/library/Libraries";
import { LibraryGrid } from "@/features/library/LibraryGrid";
import { PlayerView } from "@/features/player/PlayerView";
import { ProfileGate } from "@/features/profiles/ProfileGate";
import { ProfilePicker } from "@/features/profiles/ProfilePicker";
import { Search } from "@/features/search/Search";
import { Settings } from "@/features/settings/Settings";
import { UpdatePrompt } from "@/features/updates/UpdatePrompt";
import { toggleFrame } from "@/lib/mode";
import { useSettings } from "@/lib/settings";
import { goBack } from "@/lib/history";
import { onAction, setBackFallback } from "@/nav/input";
import { Shell } from "@/shell/Shell";
import { queryClient } from "@/lib/queryClient";

/** A new item remounts the player (fresh session state). */
function PlayerRoute() {
  const [params] = useSearchParams();
  const item = params.get("item");
  return item ? <PlayerView key={`${item}:${params.get("start") ?? ""}`} itemId={item} startMs={Number(params.get("start") ?? 0) || 0} /> : null;
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
  return (
    <QueryClientProvider client={queryClient}>
      <MotionConfig reducedMotion={intensity === 0 ? "always" : "user"}>
        <BrowserRouter>
          <GlobalActions />
          <ItemMenu />
          <ProfileGate />
          <UpdatePrompt launchReady={shown >= 0} />
          {/* One layout group: a profile's avatar flies from the picker to the sidebar. */}
          <LayoutGroup>
            <Routes>
              <Route path="/play" element={<PlayerRoute />} />
              <Route path="/profiles" element={<ProfilePicker />} />
              <Route element={<Shell />}>
                <Route path="/" element={<Home />} />
                <Route path="/libraries" element={<Libraries />} />
                <Route path="/library/:id" element={<LibraryGrid />} />
                <Route path="/item/:id" element={<Detail />} />
                <Route path="/search" element={<Search />} />
                {/* Servers moved into Settings; old links keep working. */}
                <Route path="/servers" element={<Navigate to="/settings?s=servers" replace />} />
                <Route path="/favorites" element={<Favorites />} />
                <Route path="/person/:ref" element={<PersonPage />} />
                <Route path="/settings" element={<Settings />} />
                <Route path="/admin" element={<Admin />} />
                <Route path="/debug" element={<Debug />} />
              </Route>
            </Routes>
          </LayoutGroup>
        </BrowserRouter>
        <Toaster position="bottom-center" />
        {shown !== run && <LaunchIntro key={run} skip={intensity === 0} onDone={() => setShown(run)} />}
      </MotionConfig>
    </QueryClientProvider>
  );
}
