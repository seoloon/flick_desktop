// Horizontal shelf of cards. The track pads vertically so a lifted card and
// its shadow are never clipped by the horizontal scroller.
import { motion } from "motion/react";
import type { ReactNode } from "react";
import type { MediaItem } from "@/ipc/bindings/MediaItem";
import { enter } from "@/lib/motion";
import { FocusGroup } from "@/nav/Focusable";
import { MediaCard } from "./Card";

type Props = {
  id: string;
  title: string;
  items: MediaItem[];
  shape: "poster" | "thumb";
  action?: ReactNode;
  index?: number;
};

export function Shelf({ id, title, items, shape, action, index = 0 }: Props) {
  return (
    <motion.section
      className="relative"
      initial={{ opacity: 0, y: 24 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ ...enter, delay: Math.min(index, 6) * 0.05 }}
    >
      <header className="flex items-baseline justify-between gap-4 px-[var(--gutter)]">
        <h2 className="text-[1.3125rem] font-semibold tracking-tight text-white">{title}</h2>
        {action}
      </header>
      <FocusGroup focusKey={`shelf:${id}`} fade="x" className="[--fade-size:var(--gutter)] no-scrollbar -mt-2 flex gap-[var(--card-gap)] overflow-x-auto px-[var(--gutter)] pt-6 pb-10">
        {items.map((item) => (
          <MediaCard key={item.id} item={item} shape={shape} focusKey={`shelf:${id}/${item.id}`} />
        ))}
      </FocusGroup>
    </motion.section>
  );
}
