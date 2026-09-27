import { QueryClientProvider } from "@tanstack/react-query";
import { MotionConfig } from "motion/react";
import { useEffect } from "react";
import { BrowserRouter, Route, Routes, useNavigate, useSearchParams } from "react-router";
import { Toaster } from "@/components/ui/sonner";
import { Admin } from "@/features/admin/Admin";
import { Debug } from "@/features/debug/Debug";
import { Detail } from "@/features/detail/Detail";
import { Home } from "@/features/home/Home";
import { Libraries } from "@/features/library/Libraries";
import { LibraryGrid } from "@/features/library/LibraryGrid";
import { PlayerView } from "@/features/player/PlayerView";
import { Search } from "@/features/search/Search";
import { Servers } from "@/features/servers/Servers";
import { Settings } from "@/features/settings/Settings";
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
  return (
    <QueryClientProvider client={queryClient}>
      <MotionConfig reducedMotion={intensity === 0 ? "always" : "user"}>
        <BrowserRouter>
          <GlobalActions />
          <Routes>
            <Route path="/play" element={<PlayerRoute />} />
            <Route element={<Shell />}>
              <Route path="/" element={<Home />} />
              <Route path="/libraries" element={<Libraries />} />
              <Route path="/library/:id" element={<LibraryGrid />} />
              <Route path="/item/:id" element={<Detail />} />
              <Route path="/search" element={<Search />} />
              <Route path="/servers" element={<Servers />} />
              <Route path="/settings" element={<Settings />} />
              <Route path="/admin" element={<Admin />} />
              <Route path="/debug" element={<Debug />} />
            </Route>
          </Routes>
        </BrowserRouter>
        <Toaster position="bottom-center" />
      </MotionConfig>
    </QueryClientProvider>
  );
}
