// Desktop navigation: a floating glass sidebar over the artwork (macOS
// style). Choosing an item keeps focus here, like a tvOS tab bar; Right moves
// into the content.
import { Maximize2, PanelLeftClose, PanelLeftOpen } from "lucide-react";
import { motion } from "motion/react";
import type { CSSProperties, ReactNode } from "react";
import { useNavigate } from "react-router";
import { focusSpring, pillSpring } from "@/lib/motion";
import { toggleFrame } from "@/lib/mode";
import { toggleSidebar, useSidebar } from "@/lib/sidebar";
import { cn } from "@/lib/utils";
import { FlickWordmark, MARK_WIDTH, WORDMARK_HEIGHT, WORDMARK_WIDTH } from "@/components/tv/FlickWordmark";
import { SidebarProfile } from "@/features/profiles/ProfileSwitcher";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { NAV_KEY } from "@/nav/spatial";
import { type NavItem, useNavItems } from "./navItems";

export function Sidebar() {
  const items = useNavItems();
  const collapsed = useSidebar((s) => s.collapsed);
  const active = items.find((i) => i.active);
  return (
    <aside className="fixed inset-y-0 left-0 z-30 w-[var(--sidebar-w)] p-3 transition-[width] duration-500 ease-[cubic-bezier(0.32,0.72,0,1)]">
      <FocusGroup
        focusKey={NAV_KEY}
        remember={false}
        preferredChildFocusKey={active ? `nav:${active.id}` : undefined}
        // 28 px panel, 12 px padding: items use 16 px so their corners stay concentric.
        className="glass flex h-full flex-col gap-1 overflow-hidden rounded-[1.75rem] p-3"
      >
        <Brand />
        <SidebarProfile />
        <nav aria-label="Main" className="flex flex-col gap-0.5">
          {items.map((item) => (
            <SidebarItem key={item.id} item={item} />
          ))}
        </nav>
        <div className="mt-auto flex flex-col gap-0.5">
          <SidebarButton
            id="collapse"
            label={collapsed ? "Expand Sidebar" : "Collapse Sidebar"}
            icon={collapsed ? <PanelLeftOpen /> : <PanelLeftClose />}
            onClick={toggleSidebar}
          />
          <SidebarButton id="frame" label="Flick Frame" icon={<Maximize2 />} onClick={() => void toggleFrame()} />
        </div>
      </FocusGroup>
    </aside>
  );
}

// Collapsed, the wordmark is clipped to its mark: the mark stays put and the
// lettering slides under the edge, in step with the sidebar's own width.
const brandWidths = {
  "--wordmark-w": `calc(var(--wordmark-h) * ${WORDMARK_WIDTH / WORDMARK_HEIGHT})`,
  "--mark-w": `calc(var(--wordmark-h) * ${MARK_WIDTH / WORDMARK_HEIGHT})`,
} as CSSProperties;

function Brand() {
  return (
    <div className="mb-5 flex items-center px-3 pt-2 in-data-[platform=mac]:pt-9">
      <span
        style={brandWidths}
        className="block w-(--wordmark-w) overflow-hidden transition-[width] duration-500 ease-[cubic-bezier(0.32,0.72,0,1)] [--wordmark-h:1.2rem] in-data-[sidebar=collapsed]:w-(--mark-w)"
      >
        <FlickWordmark title="Flick" className="h-(--wordmark-h) w-auto max-w-none" />
      </span>
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
  // Collapsed, the label is hidden: the tooltip says what the icon is.
  const collapsed = useSidebar((s) => s.collapsed);
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      aria-current={active ? "page" : undefined}
      aria-label={label}
      title={collapsed ? label : undefined}
      onClick={onClick}
      animate={{ scale: tv.showFocus ? 1.04 : 1 }}
      transition={focusSpring}
      className={cn(
        "relative flex h-10 w-full cursor-pointer items-center gap-3 rounded-[1rem] px-3 text-[0.9375rem] font-medium transition-colors duration-200 [&_svg]:size-[1.15rem] [&_svg]:shrink-0",
        tv.showFocus ? "text-black" : active ? "text-white" : "text-white/65 hover:bg-white/[0.07] hover:text-white",
      )}
    >
      {active && !tv.showFocus && <motion.span layoutId="sidebar-active" className="absolute inset-0 rounded-[1rem] bg-white/14" transition={pillSpring} />}
      {tv.showFocus && <motion.span layoutId="sidebar-focus" className="absolute inset-0 rounded-[1rem] bg-white shadow-[0_10px_30px_-10px_rgb(0_0_0/0.7)]" transition={pillSpring} />}
      <span className="relative flex items-center gap-3">
        {icon}
        <span className="whitespace-nowrap transition-opacity delay-200 duration-300 in-data-[sidebar=collapsed]:opacity-0 in-data-[sidebar=collapsed]:delay-0 in-data-[sidebar=collapsed]:duration-150">{label}</span>
      </span>
    </motion.button>
  );
}
