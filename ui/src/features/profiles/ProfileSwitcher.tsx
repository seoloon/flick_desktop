// Quick switch: the active profile in the sidebar (or the Flick Frame tab
// bar) opens a glass popover of the other people. A Flick PIN is typed in
// place; anything else to type (Plex PIN, sign-ins) goes through the picker.
import { useQuery } from "@tanstack/react-query";
import { ChevronsUpDown, Lock, Settings2, Users } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, type RefObject, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useNavigate } from "react-router";
import { toast } from "sonner";
import { AccountPills } from "@/components/tv/AccountPills";
import { type PinResult, PinPad } from "@/components/tv/PinPad";
import { ProfileAvatar } from "@/components/tv/ProfileAvatar";
import { api, asError } from "@/ipc/api";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import type { ProfilesState } from "@/ipc/bindings/ProfilesState";
import { focusSpring, panelSpring } from "@/lib/motion";
import { needsPlexPin, pendingSignIns, pinError, profilesQuery, switchProfile, useProfileSwitch, visibleProfiles } from "@/lib/profiles";
import { useSidebar } from "@/lib/sidebar";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { onAction } from "@/nav/input";
import { focusKey } from "@/nav/spatial";

function useActive() {
  const data = useQuery(profilesQuery).data;
  const active = data?.enabled ? data.profiles.find((p) => p.id === data.active) : undefined;
  return { data, active };
}

// The focusable only exists while there is a profile to show: a registered
// focus target without an element would trap arrow navigation.
export function SidebarProfile() {
  const { data, active } = useActive();
  return data && active ? <SidebarProfileButton data={data} active={active} /> : null;
}

export function TabBarProfile() {
  const { data, active } = useActive();
  return data && active ? <TabBarProfileButton data={data} active={active} /> : null;
}

function SidebarProfileButton({ data, active }: { data: ProfilesState; active: ProfileCard }) {
  const [open, setOpen] = useState(false);
  const collapsed = useSidebar((s) => s.collapsed);
  const tv = useTv<HTMLButtonElement>({ focusKey: "nav:profile", scroll: false });
  return (
    <>
      <motion.button
        ref={tv.ref}
        type="button"
        {...tv.props}
        onClick={() => setOpen(true)}
        title={collapsed ? active.name : undefined}
        aria-label={`Profile: ${active.name}. Switch profile`}
        animate={{ scale: tv.showFocus ? 1.04 : 1 }}
        transition={focusSpring}
        className={cn(
          "mb-3 flex h-11 w-full cursor-pointer items-center gap-3 rounded-[1rem] px-2 text-left transition-colors",
          tv.showFocus ? "bg-white text-black" : "hover:bg-white/[0.07]",
        )}
      >
        <ProfileAvatar profile={active} layoutId={`profile-avatar-${active.id}`} className="size-7 text-[0.7rem]" />
        <span className="min-w-0 flex-1 truncate text-[0.9375rem] font-medium transition-opacity delay-200 duration-300 in-data-[sidebar=collapsed]:opacity-0 in-data-[sidebar=collapsed]:delay-0">
          {active.name}
        </span>
        <ChevronsUpDown className="size-4 shrink-0 opacity-50 in-data-[sidebar=collapsed]:opacity-0" />
      </motion.button>
      <ProfilePopover open={open} anchor={tv.ref} side="left" state={data} onClose={() => setOpen(false)} />
    </>
  );
}

function TabBarProfileButton({ data, active }: { data: ProfilesState; active: ProfileCard }) {
  const [open, setOpen] = useState(false);
  const tv = useTv<HTMLButtonElement>({ focusKey: "nav:profile", scroll: false });
  return (
    <>
      <motion.button
        ref={tv.ref}
        type="button"
        {...tv.props}
        onClick={() => setOpen(true)}
        aria-label={`Profile: ${active.name}. Switch profile`}
        animate={{ scale: tv.showFocus ? 1.12 : 1 }}
        transition={focusSpring}
        className={cn("ml-1 grid size-11 cursor-pointer place-items-center rounded-full", tv.showFocus && "ring-2 ring-white")}
      >
        <ProfileAvatar profile={active} layoutId={`profile-avatar-${active.id}`} className="size-9 text-xs" />
      </motion.button>
      <ProfilePopover open={open} anchor={tv.ref} side="right" state={data} onClose={() => setOpen(false)} />
    </>
  );
}

function ProfilePopover({
  open,
  anchor,
  side,
  state,
  onClose,
}: {
  open: boolean;
  anchor: RefObject<HTMLElement | null>;
  side: "left" | "right";
  state: ProfilesState;
  onClose: () => void;
}) {
  const navigate = useNavigate();
  const [pinFor, setPinFor] = useState<ProfileCard | null>(null);
  const switching = useProfileSwitch((s) => s.phase !== "idle");
  const [pos, setPos] = useState({ top: 0, left: 0, right: 0 });
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    if (!open) return;
    const r = anchor.current?.getBoundingClientRect();
    if (r) setPos({ top: r.bottom + 8, left: r.left, right: window.innerWidth - r.right });
    setPinFor(null);
    return onAction((a) => {
      if (a.type !== "back") return false;
      close.current();
      focusKey("nav:profile");
      return true;
    });
  }, [open, anchor]);

  const others = visibleProfiles(state.profiles).filter((p) => p.id !== state.active);

  const leaveTo = (path: string) => {
    onClose();
    navigate(path);
  };

  const go = async (card: ProfileCard, pin?: string) => {
    onClose();
    try {
      const outcome = await switchProfile(card.id, { pin });
      navigate("/");
      if (outcome.failed.length) toast(`${outcome.failed.join(", ")} did not answer`);
    } catch (e) {
      toast.error(asError(e).message);
    }
  };

  const pick = (card: ProfileCard) => {
    if (switching) return;
    if (needsPlexPin(card) || pendingSignIns(card).length > 0) {
      onClose();
      navigate(`/profiles?pick=${encodeURIComponent(card.id)}`);
    } else if (card.locked) setPinFor(card);
    else void go(card);
  };

  const onPin = async (pin: string): Promise<PinResult> => {
    if (!pinFor) return "wrong";
    try {
      await api.profileCheckPin(pinFor.id, pin);
      void go(pinFor, pin);
      return "ok";
    } catch (e) {
      return pinError(e) ?? "wrong";
    }
  };

  return createPortal(
    <AnimatePresence>
      {open && (
        <>
          <motion.div key="scrim" className="fixed inset-0 z-40" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onClick={onClose} />
          <motion.div
            key="panel"
            layout
            initial={{ opacity: 0, scale: 0.92, y: -6 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: -4 }}
            transition={panelSpring}
            style={side === "left" ? { top: pos.top, left: pos.left, transformOrigin: "top left" } : { top: pos.top, right: pos.right, transformOrigin: "top right" }}
            className="glass-strong fixed z-50 w-[21rem] overflow-hidden rounded-[1.75rem] p-2 text-white"
          >
            <FocusGroup focusKey="profile-popover" boundary autoFocus className="flex flex-col gap-0.5">
              {pinFor ? (
                <div className="p-4">
                  <PinPad title={`Enter ${pinFor.name}’s PIN`} onSubmit={onPin} onCancel={() => setPinFor(null)} />
                </div>
              ) : (
                <>
                  {others.map((p) => (
                    <PopoverRow key={p.id} onClick={() => pick(p)} icon={<ProfileAvatar profile={p} className="size-10 text-sm" />}>
                      <span className="flex min-w-0 flex-1 flex-col items-start gap-1">
                        <span className="flex items-center gap-1.5 font-semibold">
                          {p.name}
                          {p.locked && <Lock className="size-3.5 opacity-60" />}
                        </span>
                        <AccountPills accounts={p.accounts} className="justify-start" />
                      </span>
                    </PopoverRow>
                  ))}
                  {others.length > 0 && <div className="mx-3 my-1.5 h-px bg-white/10" />}
                  <PopoverRow icon={<Users className="size-5" />} onClick={() => leaveTo("/profiles")}>
                    All Profiles…
                  </PopoverRow>
                  <PopoverRow icon={<Settings2 className="size-5" />} onClick={() => leaveTo("/settings?s=profiles")}>
                    Manage Profiles
                  </PopoverRow>
                </>
              )}
            </FocusGroup>
          </motion.div>
        </>
      )}
    </AnimatePresence>,
    document.body,
  );
}

function PopoverRow({ icon, children, onClick }: { icon: ReactNode; children: ReactNode; onClick: () => void }) {
  const tv = useTv<HTMLButtonElement>({ scroll: false });
  return (
    <button
      ref={tv.ref}
      type="button"
      {...tv.props}
      onClick={onClick}
      className={cn(
        // 28 px panel, 8 px padding: rows use 20 px so the corners stay concentric.
        "flex min-h-12 w-full cursor-pointer items-center gap-3 rounded-[1.25rem] px-3 py-2 text-left text-[0.9375rem] transition-colors",
        tv.showFocus ? "bg-white text-black [&_span]:text-black/70" : "hover:bg-white/[0.08]",
      )}
    >
      <span className="grid w-10 shrink-0 place-items-center">{icon}</span>
      {children}
    </button>
  );
}
