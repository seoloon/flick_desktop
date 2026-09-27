// Flick Frame navigation: the tvOS tab bar. It floats at the top on glass and
// slides away once you scroll into the content; moving focus up brings it back.
import { Minimize2 } from "lucide-react";
import { motion } from "motion/react";
import { type ReactNode, useState } from "react";
import { useNavigate } from "react-router";
import { focusSpring, panelSpring, pillSpring } from "@/lib/motion";
import { toggleFrame } from "@/lib/mode";
import { cn } from "@/lib/utils";
import { FlickMark } from "@/components/tv/FlickMark";
import { TabBarProfile } from "@/features/profiles/ProfileSwitcher";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { NAV_KEY } from "@/nav/spatial";
import { useNavItems } from "./navItems";

const ICON_ONLY = new Set(["search", "settings"]);

export function TabBar({ scrolled }: { scrolled: boolean }) {
  const items = useNavItems();
  const [focusInside, setFocusInside] = useState(false);
  const hidden = scrolled && !focusInside;
  const navigate = useNavigate();
  const active = items.find((i) => i.active);
  return (
    <motion.div
      className="pointer-events-none fixed inset-x-0 top-0 z-30 flex justify-center pt-6"
      animate={{ y: hidden ? "-140%" : "0%" }}
      transition={panelSpring}
    >
      <FocusGroup
        focusKey={NAV_KEY}
        remember={false}
        preferredChildFocusKey={active ? `nav:${active.id}` : undefined}
        onFocusWithin={setFocusInside}
        className={cn("glass pointer-events-auto flex items-center gap-1 rounded-full p-1.5 transition-opacity duration-300", hidden && "opacity-0")}
      >
        <FlickMark title="Flick" className="mr-2 ml-4 h-5 w-auto text-white/90" />
        {items.map((item) => (
          <Tab key={item.id} id={item.id} label={item.label} icon={ICON_ONLY.has(item.id) ? <item.icon /> : undefined} active={item.active} onClick={() => navigate(item.href)} />
        ))}
        <TabBarProfile />
        <Tab id="frame" label="Exit Flick Frame" icon={<Minimize2 />} onClick={() => void toggleFrame()} />
      </FocusGroup>
    </motion.div>
  );
}

function Tab({ id, label, icon, active, onClick }: { id: string; label: string; icon?: ReactNode; active?: boolean; onClick: () => void }) {
  const tv = useTv<HTMLButtonElement>({ focusKey: `nav:${id}`, scroll: false });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      aria-label={icon ? label : undefined}
      aria-current={active ? "page" : undefined}
      onClick={onClick}
      animate={{ scale: tv.showFocus ? 1.08 : 1 }}
      transition={focusSpring}
      className={cn(
        "relative flex h-11 cursor-pointer items-center justify-center rounded-full text-base font-semibold transition-colors duration-200 [&_svg]:size-5",
        icon ? "w-11" : "px-5",
        tv.showFocus ? "text-black" : active ? "text-white" : "text-white/60 hover:text-white",
      )}
    >
      {active && !tv.showFocus && <motion.span layoutId="tab-active" className="absolute inset-0 rounded-full bg-white/16" transition={pillSpring} />}
      {tv.showFocus && <motion.span layoutId="tab-focus" className="absolute inset-0 rounded-full bg-white shadow-lg" transition={pillSpring} />}
      <span className="relative">{icon ?? label}</span>
    </motion.button>
  );
}
