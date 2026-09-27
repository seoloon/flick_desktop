// Application frame: ambient backdrop, navigation (sidebar on desktop, tab
// bar in Flick Frame) and the scrolling screen. The player lives outside it.
import { SpatialNavigation } from "@noriginmedia/norigin-spatial-navigation";
import { motion } from "motion/react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Outlet, useLocation } from "react-router";
import { useMode } from "@/lib/mode";
import { cn } from "@/lib/utils";
import { onAction } from "@/nav/input";
import { focusKey, NAV_KEY, SCREEN_KEY } from "@/nav/spatial";
import { useScrollFade } from "@/nav/useScrollFade";
import ProgressiveBlur from "@/components/smoothui/progressive-blur";
import { AmbientBackdrop } from "./AmbientBackdrop";
import { Sidebar } from "./Sidebar";
import { TabBar } from "./TabBar";
import { TitleBar } from "./TitleBar";

/** Top-level destinations: Back from their content goes to the navigation. */
const TOP_LEVEL = ["/", "/libraries", "/search", "/servers", "/settings", "/admin"];

const scrollMemory = new Map<string, number>();

/**
 * Soft top/bottom edges for the screen scroller. It cannot be masked: a mask
 * would make it a backdrop root and cut the glass panels inside it off from
 * the ambient artwork. So a progressive blur and a light veil sit over it,
 * shown by the `data-fade-*` attributes of the preceding <main> (peer).
 */
function ScrollEdge({ side }: { side: "top" | "bottom" }) {
  const top = side === "top";
  return (
    <div
      aria-hidden
      className={cn(
        "pointer-events-none fixed inset-x-0 z-20 opacity-0 transition-opacity duration-500",
        top ? "top-0 h-20 peer-data-fade-start:opacity-100" : "bottom-0 h-16 peer-data-fade-end:opacity-100",
      )}
    >
      <ProgressiveBlur direction={top ? "top" : "bottom"} blur={14} layers={5} fadeIn={false} />
      <div className={cn("absolute inset-0", top ? "bg-[linear-gradient(to_bottom,rgb(0_0_0/0.4),transparent)]" : "bg-[linear-gradient(to_top,rgb(0_0_0/0.35),transparent)]")} />
    </div>
  );
}

export function Shell() {
  const location = useLocation();
  const frame = useMode((s) => s.frame);
  const main = useRef<HTMLElement>(null);
  const [scrolled, setScrolled] = useState(false);
  useScrollFade(main, "y");

  // Back on a top-level screen (tvOS Menu): focus the navigation and return
  // to the top instead of leaving the app's history.
  useEffect(() => {
    return onAction((a) => {
      if (a.type !== "back" || !TOP_LEVEL.includes(location.pathname)) return false;
      const current = SpatialNavigation.getCurrentFocusKey();
      if (current && SpatialNavigation.isDescendantOf(current, SCREEN_KEY)) {
        focusKey(NAV_KEY);
        main.current?.scrollTo({ top: 0, behavior: "smooth" });
        return true;
      }
      return location.pathname === "/"; // nothing to go back to from Home
    });
  }, [location.pathname]);

  // Each history entry keeps its scroll position.
  useLayoutEffect(() => {
    const el = main.current;
    if (el) el.scrollTop = scrollMemory.get(location.key) ?? 0;
    setScrolled((el?.scrollTop ?? 0) > 40);
  }, [location.key]);

  return (
    <div className="relative h-full">
      <AmbientBackdrop />
      {frame ? <TabBar scrolled={scrolled} /> : <Sidebar />}
      <TitleBar onWheel={(e) => main.current?.scrollBy({ top: e.deltaY })} />
      <main
        ref={main}
        onScroll={(e) => {
          const top = e.currentTarget.scrollTop;
          scrollMemory.set(location.key, top);
          setScrolled(top > 40);
        }}
        className="peer no-scrollbar relative h-full overflow-x-hidden overflow-y-auto pl-[var(--content-left)]"
      >
        <motion.div key={location.pathname} initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.35, ease: [0.32, 0.72, 0, 1] }}>
          <Outlet />
        </motion.div>
      </main>
      <ScrollEdge side="top" />
      <ScrollEdge side="bottom" />
    </div>
  );
}
