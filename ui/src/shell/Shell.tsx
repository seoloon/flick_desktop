// Application frame: ambient backdrop, navigation (sidebar on desktop, tab
// bar in Flick Frame) and the scrolling screen. The player lives outside it.
import { SpatialNavigation } from "@noriginmedia/norigin-spatial-navigation";
import { motion } from "motion/react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Outlet, useLocation } from "react-router";
import { useMode } from "@/lib/mode";
import { finishSwitch, useProfileSwitch } from "@/lib/profiles";
import { cn } from "@/lib/utils";
import { onAction } from "@/nav/input";
import { focusKey, NAV_KEY, SCREEN_KEY } from "@/nav/spatial";
import { useScrollFade } from "@/nav/useScrollFade";
import { AmbientBackdrop } from "./AmbientBackdrop";
import { Sidebar } from "./Sidebar";
import { TabBar } from "./TabBar";
import { TitleBar } from "./TitleBar";

/** Top-level destinations: Back from their content goes to the navigation. */
const TOP_LEVEL = ["/", "/libraries", "/favorites", "/search", "/settings", "/admin"];

const scrollMemory = new Map<string, number>();

/**
 * Soft top/bottom edges for the screen scroller. It cannot be masked: a mask
 * would make it a backdrop root and cut the glass panels inside it off from
 * the ambient artwork. So stacked blur layers and a light veil sit over it.
 *
 * Shown by the `data-fade-*` attributes of the preceding <main> (peer),
 * relayed as `--edge-on`. Each layer fades its *own* opacity: fading a parent
 * instead would make Chromium drop the blur until the fade ends (backdrop
 * filters only see inside a translucent ancestor), which showed as a plain
 * dark gradient snapping to blur.
 */
const EDGE_BLURS = [1, 2, 4, 8, 16];

function edgeMask(top: boolean, i: number) {
  // Stops run past 100 % for the last layers, so the strongest blur stays
  // opaque right up to the window edge instead of fading out on it.
  const step = 100 / EDGE_BLURS.length;
  const stops = `transparent ${i * step}%, #000 ${(i + 1) * step}%, #000 ${(i + 2) * step}%, transparent ${(i + 3) * step}%`;
  return `linear-gradient(to ${top ? "top" : "bottom"}, ${stops})`;
}

function ScrollEdge({ side }: { side: "top" | "bottom" }) {
  const top = side === "top";
  const layer = "absolute inset-0 transition-opacity duration-500 ease-[cubic-bezier(0.32,0.72,0,1)]";
  return (
    <div
      aria-hidden
      className={cn(
        "pointer-events-none fixed inset-x-0 z-20 [--edge-on:0]",
        top ? "top-0 h-20 peer-data-fade-start:[--edge-on:1]" : "bottom-0 h-16 peer-data-fade-end:[--edge-on:1]",
      )}
    >
      {EDGE_BLURS.map((blur, i) => (
        <div
          key={blur}
          className={layer}
          style={{
            opacity: "var(--edge-on)",
            backdropFilter: `blur(${blur}px)`,
            WebkitBackdropFilter: `blur(${blur}px)`,
            maskImage: edgeMask(top, i),
            WebkitMaskImage: edgeMask(top, i),
          }}
        />
      ))}
      <div
        className={cn(layer, top ? "bg-[linear-gradient(to_bottom,rgb(0_0_0/0.4),transparent)]" : "bg-[linear-gradient(to_top,rgb(0_0_0/0.35),transparent)]")}
        style={{ opacity: "var(--edge-on)" }}
      />
    </div>
  );
}

export function Shell() {
  const location = useLocation();
  const frame = useMode((s) => s.frame);
  const phase = useProfileSwitch((s) => s.phase);
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
        {/* No fade here: an ancestor's opacity would switch off every glass
            panel's blur until it ends. Screens fade their own pieces in.
            The one exception is a profile switch (~200 ms), when glass
            panels briefly losing their blur is acceptable. */}
        <motion.div
          key={location.pathname}
          // After a switch the new profile's screen fades in; ordinary route
          // changes only slide (no fade: see the note above).
          initial={phase === "entering" ? { y: 10, opacity: 0 } : { y: 10 }}
          animate={
            phase === "leaving"
              ? { y: 0, opacity: 0, filter: "blur(12px)" }
              : // Back to `none`: a leftover filter would cut glass panels off the artwork.
                { y: 0, opacity: 1, filter: "blur(0px)", transitionEnd: { filter: "none" } }
          }
          transition={phase === "leaving" ? { duration: 0.2, ease: [0.4, 0, 1, 1] } : { duration: 0.45, ease: [0.32, 0.72, 0, 1] }}
          onAnimationComplete={() => phase === "entering" && finishSwitch()}
        >
          <Outlet />
        </motion.div>
      </main>
      <ScrollEdge side="top" />
      <ScrollEdge side="bottom" />
    </div>
  );
}
