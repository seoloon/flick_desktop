// One person on the picker: big avatar that lifts like a tvOS poster, the
// name, where their accounts come from.
import { Lock } from "lucide-react";
import { motion } from "motion/react";
import { type ReactNode, useState } from "react";
import { AccountPills } from "@/components/tv/AccountPills";
import { ProfileAvatar } from "@/components/tv/ProfileAvatar";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import { focusSpring, panelSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { useTv } from "@/nav/Focusable";

type Props = {
  card: ProfileCard;
  index: number;
  dimmed: boolean;
  onSelect: () => void;
  onEdit?: () => void;
  onFocused: () => void;
  /** Drawn around the avatar (the loading ring while this profile opens). */
  overlay?: ReactNode;
};

export function ProfileTile({ card, index, dimmed, onSelect, onEdit, onFocused, overlay }: Props) {
  const tv = useTv<HTMLButtonElement>({ focusKey: `profile:${card.id}`, onFocused, scroll: false });
  const [hover, setHover] = useState(false);
  const [sheen, setSheen] = useState({ x: 30, y: 20 });
  const lifted = tv.showFocus || hover;
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      onClick={onSelect}
      onContextMenu={(e) => {
        e.preventDefault();
        onEdit?.();
      }}
      onPointerEnter={() => {
        setHover(true);
        onFocused();
      }}
      onPointerLeave={() => setHover(false)}
      onPointerMove={(e) => {
        const r = e.currentTarget.getBoundingClientRect();
        setSheen({ x: ((e.clientX - r.left) / r.width) * 100, y: ((e.clientY - r.top) / r.height) * 100 });
      }}
      initial={{ opacity: 0, y: 24, scale: 0.92, filter: "blur(8px)" }}
      animate={{ opacity: dimmed ? 0.55 : 1, y: 0, scale: 1, filter: "blur(0px)", transitionEnd: { filter: "none" } }}
      exit={{ opacity: 0, scale: 0.9, filter: "blur(6px)" }}
      transition={{ ...panelSpring, delay: index * 0.06 }}
      aria-label={`${card.name}${card.locked ? ", PIN protected" : ""}`}
      className="flex w-[11rem] cursor-pointer flex-col items-center gap-4 outline-none"
    >
      <motion.div
        animate={{ scale: lifted ? 1.1 : 1, y: lifted ? -6 : 0 }}
        transition={focusSpring}
        className="relative rounded-full"
        style={{ boxShadow: lifted ? "0 30px 60px -18px rgb(0 0 0 / 0.85)" : "0 12px 30px -16px rgb(0 0 0 / 0.6)" }}
      >
        <ProfileAvatar profile={card} layoutId={`profile-avatar-${card.id}`} className="size-36 text-5xl" />
        <span
          aria-hidden
          className={cn("pointer-events-none absolute inset-0 rounded-full transition-opacity duration-300", lifted ? "opacity-100" : "opacity-0")}
          style={{ background: `radial-gradient(60% 60% at ${sheen.x}% ${sheen.y}%, rgb(255 255 255 / 0.10), transparent 70%)` }}
        />
        <span aria-hidden className={cn("pointer-events-none absolute -inset-1 rounded-full ring-white transition-[box-shadow] duration-200", tv.showFocus ? "ring-4" : "ring-0")} />
        {overlay}
        {card.locked && (
          <span className="absolute right-1 bottom-1 grid size-8 place-items-center rounded-full bg-black/70 text-white/90">
            <Lock className="size-4" />
          </span>
        )}
      </motion.div>
      <span className={cn("max-w-full truncate text-lg font-semibold transition-colors", lifted ? "text-white" : "text-white/80")}>{card.name}</span>
      <AccountPills accounts={card.accounts} className="-mt-2" />
    </motion.button>
  );
}
