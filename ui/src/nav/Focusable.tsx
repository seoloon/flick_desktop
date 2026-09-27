// React side of spatial navigation. Every interactive element is a native
// element registered with Norigin through `useTv`; containers (`FocusGroup`)
// remember their last focused child, which is what makes shelves feel right
// on a TV. `Screen` is the root of each route and restores focus on return.
import {
  FocusContext,
  getCurrentFocusKey,
  SpatialNavigation,
  useFocusable,
  type UseFocusableConfig,
} from "@noriginmedia/norigin-spatial-navigation";
import { type ComponentProps, type FocusEvent, type ReactNode, useEffect, useRef } from "react";
import { useLocation } from "react-router";
import { cn } from "@/lib/utils";
import { useModality } from "./input";
import { SCREEN_KEY } from "./spatial";
import { type FadeAxis, useScrollFade } from "./useScrollFade";

const reducedMotion = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

type TvOptions = Omit<UseFocusableConfig<object>, "extraProps"> & {
  /** Take focus when mounted. */
  autoFocus?: boolean;
  /** How to bring the element into view when it gains focus. */
  scroll?: ScrollLogicalPosition | false;
  /** Called on every focus, remote or pointer (drives the ambient backdrop). */
  onFocused?: () => void;
};

/**
 * Registers a native element as a focus target. Spread `props` onto it.
 * `showFocus` is true only while a remote/keyboard/controller drives the UI,
 * so mouse users never see TV focus visuals stuck on the last clicked item.
 */
export function useTv<E extends HTMLElement = HTMLElement>({ autoFocus, scroll = "nearest", onFocused, onFocus, ...config }: TvOptions = {}) {
  const onFocusedRef = useRef(onFocused);
  onFocusedRef.current = onFocused;
  const f = useFocusable<object, E>({
    ...config,
    onFocus: (layout, props, details) => {
      if (scroll && useModality.getState().modality === "keys") {
        layout.node.scrollIntoView({ block: scroll, inline: "nearest", behavior: reducedMotion() ? "auto" : "smooth" });
      }
      onFocusedRef.current?.();
      onFocus?.(layout, props, details);
    },
  });
  const modality = useModality((s) => s.modality);

  useEffect(() => {
    if (autoFocus) f.focusSelf();
    // Mount only: autoFocus is an initial preference.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return {
    ref: f.ref,
    focused: f.focused,
    showFocus: f.focused && modality === "keys",
    focusSelf: f.focusSelf,
    focusKey: f.focusKey,
    props: {
      "data-tv": "",
      // Pointer or Tab focus: tell Norigin, so arrows continue from here.
      onFocus: (e: FocusEvent<E>) => {
        if (e.target === e.currentTarget && getCurrentFocusKey() !== f.focusKey) f.focusSelf();
      },
    },
  };
}

type GroupProps = Omit<ComponentProps<"div">, "ref"> & {
  focusKey?: string;
  /** Re-entering the group lands on its last focused child (default). */
  remember?: boolean;
  /** Focus cannot leave the group (dialogs, panels). */
  boundary?: boolean;
  preferredChildFocusKey?: string;
  autoFocus?: boolean;
  /** Told when focus enters or leaves the group. */
  onFocusWithin?: (inside: boolean) => void;
  /** The group scrolls on this axis: fade its edges where content continues. */
  fade?: FadeAxis;
  children: ReactNode;
};

/** A navigable container: a shelf, a toolbar, a list. */
export function FocusGroup({ focusKey, remember = true, boundary, preferredChildFocusKey, autoFocus, onFocusWithin, fade, children, className, ...rest }: GroupProps) {
  const { ref, focusKey: key, hasFocusedChild, focusSelf } = useFocusable<object, HTMLDivElement>({
    focusKey,
    trackChildren: true,
    saveLastFocusedChild: remember,
    isFocusBoundary: boundary,
    preferredChildFocusKey,
  });
  useEffect(() => {
    if (autoFocus) focusSelf();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const within = useRef(onFocusWithin);
  within.current = onFocusWithin;
  useEffect(() => within.current?.(hasFocusedChild), [hasFocusedChild]);
  useScrollFade(ref, fade);
  return (
    <FocusContext.Provider value={key}>
      <div ref={ref} className={cn(fade === "x" && "scroll-fade-x", fade === "y" && "scroll-fade-y", className)} data-focus-within={hasFocusedChild || undefined} {...rest}>
        {children}
      </div>
    </FocusContext.Provider>
  );
}

// Last focused element per history entry, so Back returns to the same card.
const remembered = new Map<string, string>();

/**
 * Root of a route. Once `ready` (content rendered), it focuses the element
 * the user left on this history entry, else its first focusable, unless focus
 * already sits in the navigation (tvOS: choosing a tab keeps focus there).
 */
export function Screen({ ready = true, className, children }: { ready?: boolean; className?: string; children: ReactNode }) {
  const location = useLocation();
  const entry = location.key;
  const { ref, focusSelf } = useFocusable<object, HTMLDivElement>({
    focusKey: SCREEN_KEY,
    trackChildren: true,
    saveLastFocusedChild: true,
  });

  useEffect(() => {
    return () => {
      const current = getCurrentFocusKey();
      if (current && SpatialNavigation.isDescendantOf(current, SCREEN_KEY)) remembered.set(entry, current);
    };
  }, [entry]);

  useEffect(() => {
    if (!ready) return;
    const id = requestAnimationFrame(() => {
      const current = getCurrentFocusKey();
      const inScreen = current && SpatialNavigation.doesFocusableExist(current) && SpatialNavigation.isDescendantOf(current, SCREEN_KEY);
      if (inScreen) return;
      const inNav = current && SpatialNavigation.doesFocusableExist(current) && !SpatialNavigation.isDescendantOf(current, SCREEN_KEY);
      const saved = remembered.get(entry);
      if (saved && SpatialNavigation.doesFocusableExist(saved)) {
        void SpatialNavigation.setFocus(saved);
        return;
      }
      if (!inNav) focusSelf();
    });
    return () => cancelAnimationFrame(id);
  }, [ready, entry, focusSelf]);

  return (
    <FocusContext.Provider value={SCREEN_KEY}>
      <div ref={ref} className={cn("min-h-full", className)}>
        {children}
      </div>
    </FocusContext.Provider>
  );
}
