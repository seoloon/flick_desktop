// A profile's round picture: the server's avatar when there is one, else
// initials on a gradient of the profile's colour. `layoutId` lets the same
// avatar fly from the picker to the sidebar.
import { motion } from "motion/react";
import { useState } from "react";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import { avatarUrl } from "@/ipc/images";
import { avatarGradient, initials } from "@/lib/profiles";
import { cn } from "@/lib/utils";

type Props = { profile: Pick<ProfileCard, "id" | "name" | "color" | "avatarKey">; className?: string; layoutId?: string };

export function ProfileAvatar({ profile, className, layoutId }: Props) {
  const [broken, setBroken] = useState(false);
  const src = profile.avatarKey && !broken ? avatarUrl(profile.id, profile.avatarKey) : undefined;
  return (
    <motion.div
      layoutId={layoutId}
      className={cn("relative grid shrink-0 place-items-center overflow-hidden rounded-full select-none", className)}
      style={{ background: avatarGradient(profile.color) }}
    >
      {src ? (
        <img src={src} alt="" draggable={false} decoding="async" onError={() => setBroken(true)} className="size-full object-cover" />
      ) : (
        <span aria-hidden className="font-heading leading-none font-bold tracking-tight text-white/95">
          {initials(profile.name)}
        </span>
      )}
    </motion.div>
  );
}
