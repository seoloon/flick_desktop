// Desktop navigation: a floating glass sidebar over the artwork (macOS
// style). Choosing an item keeps focus here, like a tvOS tab bar; Right moves
// into the content.
import { Maximize2 } from "lucide-react";
import { motion } from "motion/react";
import type { ReactNode } from "react";
import { useNavigate } from "react-router";
import { focusSpring, pillSpring } from "@/lib/motion";
import { toggleFrame } from "@/lib/mode";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { NAV_KEY } from "@/nav/spatial";
import { type NavItem, useNavItems } from "./navItems";

export function Sidebar() {
  const items = useNavItems();
  const active = items.find((i) => i.active);
  return (
    <aside className="fixed inset-y-0 left-0 z-30 w-[var(--sidebar-w)] p-3">
      <FocusGroup
        focusKey={NAV_KEY}
        remember={false}
        preferredChildFocusKey={active ? `nav:${active.id}` : undefined}
        className="glass flex h-full flex-col gap-1 rounded-[1.75rem] p-3"
      >
        <Brand />
        <nav aria-label="Main" className="flex flex-col gap-0.5">
          {items.map((item) => (
            <SidebarItem key={item.id} item={item} />
          ))}
        </nav>
        <div className="mt-auto">
          <SidebarButton id="frame" label="Flick Frame" icon={<Maximize2 />} onClick={() => void toggleFrame()} />
        </div>
      </FocusGroup>
    </aside>
  );
}

function Brand() {
  return (
    <div className="mb-5 flex items-center gap-2.5 px-3 pt-2 in-data-[platform=mac]:pt-9">
      <span className="grid size-7 place-items-center rounded-lg bg-white text-black">
        <svg viewBox="0 0 24 24" className="size-4" aria-hidden>
          <path d="M8 5.5v13l10.5-6.5z" fill="currentColor" />
        </svg>
      </span>
      <span className="font-heading text-[1.0625rem] font-bold tracking-tight">Flick</span>
    </div>
  );
}

function SidebarItem({ item }: { item: NavItem }) {
  const navigate = useNavigate();
  const Icon = item.icon;
  return <SidebarButton id={item.id} label={item.label} icon={<Icon />} active={item.active} onClick={() => navigate(item.href)} />;
}

function SidebarButton({ id, label, icon, active, onClick }: { id: string; label: string; icon: ReactNode; active?: boolean; onClick: () => void }) {
  const tv = useTv<HTMLButtonElement>({ focusKey: `nav:${id}`, scroll: false });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      aria-current={active ? "page" : undefined}
      onClick={onClick}
      animate={{ scale: tv.showFocus ? 1.04 : 1 }}
      transition={focusSpring}
      className={cn(
        "relative flex h-10 w-full cursor-pointer items-center gap-3 rounded-xl px-3 text-[0.9375rem] font-medium transition-colors duration-200 [&_svg]:size-[1.15rem] [&_svg]:shrink-0",
        tv.showFocus ? "text-black" : active ? "text-white" : "text-white/65 hover:bg-white/[0.07] hover:text-white",
      )}
    >
      {active && !tv.showFocus && <motion.span layoutId="sidebar-active" className="absolute inset-0 rounded-xl bg-white/14" transition={pillSpring} />}
      {tv.showFocus && <motion.span layoutId="sidebar-focus" className="absolute inset-0 rounded-xl bg-white shadow-[0_10px_30px_-10px_rgb(0_0_0/0.7)]" transition={pillSpring} />}
      <span className="relative flex items-center gap-3">
        {icon}
        {label}
      </span>
    </motion.button>
  );
}
